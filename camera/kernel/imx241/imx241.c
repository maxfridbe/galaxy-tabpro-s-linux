// SPDX-License-Identifier: GPL-2.0
/*
 * Sony IMX241 5MP camera sensor driver (Samsung Galaxy TabPro S, ACPI INT347F).
 *
 * Register settings were taken from the Windows imx241.sys driver shipped with
 * the Galaxy TabPro S. Power sequencing follows the Samsung TPS68470 control
 * logic (SkcController.sys). Structure modelled on drivers/media/i2c/ov5693.c.
 */
#include <linux/acpi.h>
#include <linux/clk.h>
#include <linux/delay.h>
#include <linux/gpio/consumer.h>
#include <linux/i2c.h>
#include <linux/module.h>
#include <linux/pm_runtime.h>
#include <linux/regulator/consumer.h>
#include <linux/units.h>

#include <media/v4l2-cci.h>
#include <media/v4l2-ctrls.h>
#include <media/v4l2-device.h>
#include <media/v4l2-fwnode.h>

#include "imx241_modes.h"

#define IMX241_REG_MODEL_ID		CCI_REG16(0x0000)
#define IMX241_MODEL_ID			0x0241
#define IMX241_REG_MODE_SELECT		CCI_REG8(0x0100)
#define IMX241_REG_ORIENTATION		CCI_REG8(0x0101)
#define IMX241_REG_SW_RESET		CCI_REG8(0x0103)
#define IMX241_REG_EXPOSURE		CCI_REG16(0x0202)
#define IMX241_REG_ANALOG_GAIN		CCI_REG16(0x0204)
#define IMX241_REG_DGAIN_GR		CCI_REG16(0x020e)
#define IMX241_REG_DGAIN_R		CCI_REG16(0x0210)
#define IMX241_REG_DGAIN_B		CCI_REG16(0x0212)
#define IMX241_REG_DGAIN_GB		CCI_REG16(0x0214)
#define IMX241_REG_FRAME_LENGTH		CCI_REG16(0x0340)
#define IMX241_REG_TEST_PATTERN		CCI_REG16(0x0600)

#define IMX241_XVCLK_FREQ		19200000
#define IMX241_LINK_FREQ_FULL		499200000ULL	/* 998.4 Mbps/lane */
#define IMX241_LINK_FREQ_BIN		249600000ULL	/* 499.2 Mbps/lane */
#define IMX241_LANES			2
#define IMX241_BPP			10
#define IMX241_PPL			2912	/* 2 * line_length_pck (0x05b0) */
#define IMX241_VTS_MAX			0xffff
#define IMX241_EXPOSURE_MARGIN		4
#define IMX241_AGAIN_MIN		0
#define IMX241_AGAIN_MAX		224	/* 256/(256-224) = 8x */
#define IMX241_DGAIN_MIN		0x100
#define IMX241_DGAIN_MAX		0xfff
#define IMX241_DGAIN_DEF		0x100

#define IMX241_NATIVE_WIDTH		2592
#define IMX241_NATIVE_HEIGHT		1944

static const s64 imx241_link_freqs[] = {
	IMX241_LINK_FREQ_FULL,
	IMX241_LINK_FREQ_BIN,
};

struct imx241_mode {
	u32 width;
	u32 height;
	u32 vts_def;
	unsigned int link_freq_index;
	const struct cci_reg_sequence *regs;
	unsigned int num_regs;
};

static const struct imx241_mode imx241_modes[] = {
	{
		.width = 2592, .height = 1944, .vts_def = 0x08e8,
		.link_freq_index = 0,
		.regs = imx241_mode_2592x1944_regs,
		.num_regs = ARRAY_SIZE(imx241_mode_2592x1944_regs),
	},
	{
		.width = 1296, .height = 972, .vts_def = 0x023a,
		.link_freq_index = 1,
		.regs = imx241_mode_1296x972_regs,
		.num_regs = ARRAY_SIZE(imx241_mode_1296x972_regs),
	},
};

static const char * const imx241_supply_names[] = {
	"avdd",		/* analog 2.8 V */
	"dovdd",	/* I/O 1.8 V */
	"dvdd",		/* core 1.2 V */
	"vsio",		/* sensor I2C/IO level (TPS68470 VSIO), board specific */
};
#define IMX241_NUM_SUPPLIES ARRAY_SIZE(imx241_supply_names)

static const char * const imx241_test_pattern_menu[] = {
	"Disabled",
	"Solid Colour",
	"Colour Bars",
	"Fade To Grey Colour Bars",
	"PN9",
};

struct imx241 {
	struct device *dev;
	struct regmap *regmap;
	struct v4l2_subdev sd;
	struct media_pad pad;
	struct mutex lock;	/* protects mode, ctrls, streaming */

	struct clk *xvclk;
	struct gpio_descs *enable;
	struct gpio_desc *reset;
	struct gpio_desc *powerdown;
	struct regulator_bulk_data supplies[IMX241_NUM_SUPPLIES];

	const struct imx241_mode *mode;
	u32 code;
	bool streaming;

	struct v4l2_ctrl_handler ctrl_handler;
	struct v4l2_ctrl *link_freq;
	struct v4l2_ctrl *pixel_rate;
	struct v4l2_ctrl *exposure;
	struct v4l2_ctrl *vblank;
	struct v4l2_ctrl *hblank;
	struct v4l2_ctrl *hflip;
	struct v4l2_ctrl *vflip;
};

static inline struct imx241 *to_imx241(struct v4l2_subdev *sd)
{
	return container_of(sd, struct imx241, sd);
}

static u64 imx241_pixel_rate(const struct imx241_mode *mode)
{
	return div_u64(imx241_link_freqs[mode->link_freq_index] * 2 * IMX241_LANES,
		       IMX241_BPP);
}

/* Bayer order for the current flip settings (sensor native order RGGB). */
static u32 imx241_get_code(struct imx241 *imx)
{
	static const u32 codes[2][2] = {
		{ MEDIA_BUS_FMT_SRGGB10_1X10, MEDIA_BUS_FMT_SGRBG10_1X10 },
		{ MEDIA_BUS_FMT_SGBRG10_1X10, MEDIA_BUS_FMT_SBGGR10_1X10 },
	};
	bool h = imx->hflip ? imx->hflip->val : false;
	bool v = imx->vflip ? imx->vflip->val : false;

	return codes[v][h];
}

/* ------------------------------------------------------------------------- */
/* Controls */

static int imx241_set_ctrl(struct v4l2_ctrl *ctrl)
{
	struct imx241 *imx = container_of(ctrl->handler, struct imx241, ctrl_handler);
	int ret = 0;

	if (ctrl->id == V4L2_CID_VBLANK) {
		int exp_max = imx->mode->height + ctrl->val - IMX241_EXPOSURE_MARGIN;

		__v4l2_ctrl_modify_range(imx->exposure, imx->exposure->minimum,
					 exp_max, imx->exposure->step,
					 min(imx->exposure->val, exp_max));
	}

	/* Only touch hardware while powered */
	if (!pm_runtime_get_if_in_use(imx->dev))
		return 0;

	switch (ctrl->id) {
	case V4L2_CID_EXPOSURE:
		cci_write(imx->regmap, IMX241_REG_EXPOSURE, ctrl->val, &ret);
		break;
	case V4L2_CID_ANALOGUE_GAIN:
		cci_write(imx->regmap, IMX241_REG_ANALOG_GAIN, ctrl->val, &ret);
		break;
	case V4L2_CID_DIGITAL_GAIN:
		cci_write(imx->regmap, IMX241_REG_DGAIN_GR, ctrl->val, &ret);
		cci_write(imx->regmap, IMX241_REG_DGAIN_R, ctrl->val, &ret);
		cci_write(imx->regmap, IMX241_REG_DGAIN_B, ctrl->val, &ret);
		cci_write(imx->regmap, IMX241_REG_DGAIN_GB, ctrl->val, &ret);
		break;
	case V4L2_CID_VBLANK:
		cci_write(imx->regmap, IMX241_REG_FRAME_LENGTH,
			  imx->mode->height + ctrl->val, &ret);
		break;
	case V4L2_CID_HFLIP:
	case V4L2_CID_VFLIP:
		cci_write(imx->regmap, IMX241_REG_ORIENTATION,
			  imx->hflip->val | (imx->vflip->val << 1), &ret);
		break;
	case V4L2_CID_TEST_PATTERN:
		cci_write(imx->regmap, IMX241_REG_TEST_PATTERN, ctrl->val, &ret);
		break;
	default:
		ret = -EINVAL;
	}

	pm_runtime_put(imx->dev);
	return ret;
}

static const struct v4l2_ctrl_ops imx241_ctrl_ops = {
	.s_ctrl = imx241_set_ctrl,
};

static int imx241_init_controls(struct imx241 *imx)
{
	struct v4l2_ctrl_handler *hdl = &imx->ctrl_handler;
	const struct imx241_mode *mode = imx->mode;
	struct v4l2_fwnode_device_properties props;
	u64 rate = imx241_pixel_rate(mode);
	int exp_max = mode->vts_def - IMX241_EXPOSURE_MARGIN;
	int hblank = IMX241_PPL - mode->width;
	int ret;

	ret = v4l2_ctrl_handler_init(hdl, 12);
	if (ret)
		return ret;
	hdl->lock = &imx->lock;

	imx->link_freq = v4l2_ctrl_new_int_menu(hdl, NULL, V4L2_CID_LINK_FREQ,
						ARRAY_SIZE(imx241_link_freqs) - 1,
						mode->link_freq_index,
						imx241_link_freqs);
	if (imx->link_freq)
		imx->link_freq->flags |= V4L2_CTRL_FLAG_READ_ONLY;

	imx->pixel_rate = v4l2_ctrl_new_std(hdl, NULL, V4L2_CID_PIXEL_RATE,
					    1, imx241_pixel_rate(&imx241_modes[0]),
					    1, rate);

	imx->hblank = v4l2_ctrl_new_std(hdl, NULL, V4L2_CID_HBLANK,
					hblank, hblank, 1, hblank);
	if (imx->hblank)
		imx->hblank->flags |= V4L2_CTRL_FLAG_READ_ONLY;

	imx->vblank = v4l2_ctrl_new_std(hdl, &imx241_ctrl_ops, V4L2_CID_VBLANK,
					mode->vts_def - mode->height,
					IMX241_VTS_MAX - mode->height, 1,
					mode->vts_def - mode->height);

	imx->exposure = v4l2_ctrl_new_std(hdl, &imx241_ctrl_ops, V4L2_CID_EXPOSURE,
					  1, exp_max, 1, exp_max / 2);

	v4l2_ctrl_new_std(hdl, &imx241_ctrl_ops, V4L2_CID_ANALOGUE_GAIN,
			  IMX241_AGAIN_MIN, IMX241_AGAIN_MAX, 1, 0);

	v4l2_ctrl_new_std(hdl, &imx241_ctrl_ops, V4L2_CID_DIGITAL_GAIN,
			  IMX241_DGAIN_MIN, IMX241_DGAIN_MAX, 1, IMX241_DGAIN_DEF);

	imx->hflip = v4l2_ctrl_new_std(hdl, &imx241_ctrl_ops, V4L2_CID_HFLIP, 0, 1, 1, 0);
	imx->vflip = v4l2_ctrl_new_std(hdl, &imx241_ctrl_ops, V4L2_CID_VFLIP, 0, 1, 1, 0);
	if (imx->hflip)
		imx->hflip->flags |= V4L2_CTRL_FLAG_MODIFY_LAYOUT;
	if (imx->vflip)
		imx->vflip->flags |= V4L2_CTRL_FLAG_MODIFY_LAYOUT;

	v4l2_ctrl_new_std_menu_items(hdl, &imx241_ctrl_ops, V4L2_CID_TEST_PATTERN,
				     ARRAY_SIZE(imx241_test_pattern_menu) - 1,
				     0, 0, imx241_test_pattern_menu);

	ret = v4l2_fwnode_device_parse(imx->dev, &props);
	if (ret)
		goto err;
	ret = v4l2_ctrl_new_fwnode_properties(hdl, &imx241_ctrl_ops, &props);
	if (ret)
		goto err;

	if (hdl->error) {
		ret = hdl->error;
		goto err;
	}

	imx->sd.ctrl_handler = hdl;
	return 0;
err:
	v4l2_ctrl_handler_free(hdl);
	return ret;
}

static void imx241_update_mode_ctrls(struct imx241 *imx)
{
	const struct imx241_mode *mode = imx->mode;
	int hblank = IMX241_PPL - mode->width;
	int exp_max = mode->vts_def - IMX241_EXPOSURE_MARGIN;

	__v4l2_ctrl_s_ctrl(imx->link_freq, mode->link_freq_index);
	__v4l2_ctrl_s_ctrl_int64(imx->pixel_rate, imx241_pixel_rate(mode));
	__v4l2_ctrl_modify_range(imx->hblank, hblank, hblank, 1, hblank);
	__v4l2_ctrl_modify_range(imx->vblank, mode->vts_def - mode->height,
				 IMX241_VTS_MAX - mode->height, 1,
				 mode->vts_def - mode->height);
	__v4l2_ctrl_s_ctrl(imx->vblank, mode->vts_def - mode->height);
	__v4l2_ctrl_modify_range(imx->exposure, 1, exp_max, 1, exp_max / 2);
}

/* ------------------------------------------------------------------------- */
/* Power */

static void imx241_power_off(struct imx241 *imx)
{
	int i;

	clk_disable_unprepare(imx->xvclk);
	gpiod_set_value_cansleep(imx->reset, 1);
	gpiod_set_value_cansleep(imx->powerdown, 1);
	regulator_bulk_disable(IMX241_NUM_SUPPLIES, imx->supplies);
	if (imx->enable)
		for (i = imx->enable->ndescs - 1; i >= 0; i--)
			gpiod_set_value_cansleep(imx->enable->desc[i], 0);
}

static int imx241_power_on(struct imx241 *imx)
{
	unsigned int i;
	int ret;

	gpiod_set_value_cansleep(imx->reset, 1);
	gpiod_set_value_cansleep(imx->powerdown, 1);

	/* Board-level enables (e.g. external AVDD LDO on the front camera) */
	if (imx->enable)
		for (i = 0; i < imx->enable->ndescs; i++) {
			gpiod_set_value_cansleep(imx->enable->desc[i], 1);
			usleep_range(2000, 3000);
		}

	ret = regulator_bulk_enable(IMX241_NUM_SUPPLIES, imx->supplies);
	if (ret) {
		dev_err(imx->dev, "failed to enable regulators: %d\n", ret);
		goto err;
	}
	usleep_range(2000, 3000);

	gpiod_set_value_cansleep(imx->reset, 0);
	usleep_range(2000, 3000);
	gpiod_set_value_cansleep(imx->powerdown, 0);
	usleep_range(2000, 3000);

	ret = clk_prepare_enable(imx->xvclk);
	if (ret) {
		dev_err(imx->dev, "failed to enable clock: %d\n", ret);
		goto err;
	}
	usleep_range(10000, 12000);
	return 0;
err:
	imx241_power_off(imx);
	return ret;
}

static int imx241_runtime_suspend(struct device *dev)
{
	struct v4l2_subdev *sd = dev_get_drvdata(dev);

	imx241_power_off(to_imx241(sd));
	return 0;
}

static int imx241_runtime_resume(struct device *dev)
{
	struct v4l2_subdev *sd = dev_get_drvdata(dev);

	return imx241_power_on(to_imx241(sd));
}

/* ------------------------------------------------------------------------- */
/* Streaming */

static int imx241_start_streaming(struct imx241 *imx)
{
	int ret = 0;

	cci_write(imx->regmap, IMX241_REG_SW_RESET, 0x01, &ret);
	if (ret)
		return ret;
	usleep_range(10000, 12000);

	ret = cci_multi_reg_write(imx->regmap, imx->mode->regs,
				  imx->mode->num_regs, NULL);
	if (ret)
		return ret;

	ret = __v4l2_ctrl_handler_setup(&imx->ctrl_handler);
	if (ret)
		return ret;

	cci_write(imx->regmap, IMX241_REG_MODE_SELECT, 0x01, &ret);
	return ret;
}

static int imx241_s_stream(struct v4l2_subdev *sd, int enable)
{
	struct imx241 *imx = to_imx241(sd);
	int ret = 0;

	mutex_lock(&imx->lock);
	if (enable == imx->streaming)
		goto out;

	if (enable) {
		ret = pm_runtime_resume_and_get(imx->dev);
		if (ret)
			goto out;
		ret = imx241_start_streaming(imx);
		if (ret) {
			pm_runtime_put(imx->dev);
			goto out;
		}
	} else {
		cci_write(imx->regmap, IMX241_REG_MODE_SELECT, 0x00, NULL);
		pm_runtime_put(imx->dev);
	}
	imx->streaming = enable;
	__v4l2_ctrl_grab(imx->hflip, enable);
	__v4l2_ctrl_grab(imx->vflip, enable);
out:
	mutex_unlock(&imx->lock);
	return ret;
}

/* ------------------------------------------------------------------------- */
/* Pad ops */

static void imx241_fill_fmt(struct imx241 *imx, const struct imx241_mode *mode,
			    struct v4l2_mbus_framefmt *fmt)
{
	fmt->width = mode->width;
	fmt->height = mode->height;
	fmt->code = imx241_get_code(imx);
	fmt->field = V4L2_FIELD_NONE;
	fmt->colorspace = V4L2_COLORSPACE_RAW;
	fmt->ycbcr_enc = V4L2_YCBCR_ENC_601;
	fmt->quantization = V4L2_QUANTIZATION_FULL_RANGE;
	fmt->xfer_func = V4L2_XFER_FUNC_NONE;
}

static int imx241_enum_mbus_code(struct v4l2_subdev *sd,
				 struct v4l2_subdev_state *state,
				 struct v4l2_subdev_mbus_code_enum *code)
{
	struct imx241 *imx = to_imx241(sd);

	if (code->index > 0)
		return -EINVAL;
	mutex_lock(&imx->lock);
	code->code = imx241_get_code(imx);
	mutex_unlock(&imx->lock);
	return 0;
}

static int imx241_enum_frame_size(struct v4l2_subdev *sd,
				  struct v4l2_subdev_state *state,
				  struct v4l2_subdev_frame_size_enum *fse)
{
	if (fse->index >= ARRAY_SIZE(imx241_modes))
		return -EINVAL;
	fse->min_width = fse->max_width = imx241_modes[fse->index].width;
	fse->min_height = fse->max_height = imx241_modes[fse->index].height;
	return 0;
}

static int imx241_get_fmt(struct v4l2_subdev *sd, struct v4l2_subdev_state *state,
			  struct v4l2_subdev_format *fmt)
{
	struct imx241 *imx = to_imx241(sd);

	mutex_lock(&imx->lock);
	if (fmt->which == V4L2_SUBDEV_FORMAT_TRY)
		fmt->format = *v4l2_subdev_state_get_format(state, fmt->pad);
	else
		imx241_fill_fmt(imx, imx->mode, &fmt->format);
	mutex_unlock(&imx->lock);
	return 0;
}

static int imx241_set_fmt(struct v4l2_subdev *sd, struct v4l2_subdev_state *state,
			  struct v4l2_subdev_format *fmt)
{
	struct imx241 *imx = to_imx241(sd);
	const struct imx241_mode *mode;

	mode = v4l2_find_nearest_size(imx241_modes, ARRAY_SIZE(imx241_modes),
				      width, height, fmt->format.width,
				      fmt->format.height);

	mutex_lock(&imx->lock);
	imx241_fill_fmt(imx, mode, &fmt->format);
	if (fmt->which == V4L2_SUBDEV_FORMAT_TRY) {
		*v4l2_subdev_state_get_format(state, fmt->pad) = fmt->format;
	} else if (imx->streaming) {
		mutex_unlock(&imx->lock);
		return -EBUSY;
	} else {
		imx->mode = mode;
		imx241_update_mode_ctrls(imx);
	}
	mutex_unlock(&imx->lock);
	return 0;
}

static int imx241_get_selection(struct v4l2_subdev *sd,
				struct v4l2_subdev_state *state,
				struct v4l2_subdev_selection *sel)
{
	switch (sel->target) {
	case V4L2_SEL_TGT_CROP:
	case V4L2_SEL_TGT_CROP_DEFAULT:
	case V4L2_SEL_TGT_CROP_BOUNDS:
	case V4L2_SEL_TGT_NATIVE_SIZE:
		sel->r.top = 0;
		sel->r.left = 0;
		sel->r.width = IMX241_NATIVE_WIDTH;
		sel->r.height = IMX241_NATIVE_HEIGHT;
		return 0;
	}
	return -EINVAL;
}

static int imx241_init_state(struct v4l2_subdev *sd, struct v4l2_subdev_state *state)
{
	struct imx241 *imx = to_imx241(sd);

	imx241_fill_fmt(imx, &imx241_modes[0], v4l2_subdev_state_get_format(state, 0));
	return 0;
}

static const struct v4l2_subdev_video_ops imx241_video_ops = {
	.s_stream = imx241_s_stream,
};

static const struct v4l2_subdev_pad_ops imx241_pad_ops = {
	.enum_mbus_code = imx241_enum_mbus_code,
	.enum_frame_size = imx241_enum_frame_size,
	.get_fmt = imx241_get_fmt,
	.set_fmt = imx241_set_fmt,
	.get_selection = imx241_get_selection,
};

static const struct v4l2_subdev_ops imx241_subdev_ops = {
	.video = &imx241_video_ops,
	.pad = &imx241_pad_ops,
};

static const struct v4l2_subdev_internal_ops imx241_internal_ops = {
	.init_state = imx241_init_state,
};

/* ------------------------------------------------------------------------- */
/* Probe */

static int imx241_check_hwcfg(struct imx241 *imx)
{
	struct fwnode_handle *ep;
	struct v4l2_fwnode_endpoint bus_cfg = { .bus_type = V4L2_MBUS_CSI2_DPHY };
	int ret;

	ep = fwnode_graph_get_next_endpoint(dev_fwnode(imx->dev), NULL);
	if (!ep)
		return dev_err_probe(imx->dev, -EPROBE_DEFER,
				     "waiting for fwnode graph endpoint\n");
	ret = v4l2_fwnode_endpoint_alloc_parse(ep, &bus_cfg);
	fwnode_handle_put(ep);
	if (ret)
		return ret;

	if (bus_cfg.bus.mipi_csi2.num_data_lanes != IMX241_LANES)
		dev_warn(imx->dev, "expected %d lanes, firmware says %u\n",
			 IMX241_LANES, bus_cfg.bus.mipi_csi2.num_data_lanes);
	v4l2_fwnode_endpoint_free(&bus_cfg);
	return 0;
}

static int imx241_identify(struct imx241 *imx)
{
	u64 model = 0;
	int ret = 0;

	cci_read(imx->regmap, IMX241_REG_MODEL_ID, &model, &ret);
	if (ret)
		return dev_err_probe(imx->dev, ret, "failed to read sensor id\n");
	if (model != IMX241_MODEL_ID)
		return dev_err_probe(imx->dev, -ENODEV, "unexpected model id 0x%04llx\n", model);
	dev_info(imx->dev, "Sony IMX241 detected\n");
	return 0;
}

static int imx241_probe(struct i2c_client *client)
{
	struct imx241 *imx;
	unsigned int i;
	u32 rate;
	int ret;

	imx = devm_kzalloc(&client->dev, sizeof(*imx), GFP_KERNEL);
	if (!imx)
		return -ENOMEM;
	imx->dev = &client->dev;
	imx->mode = &imx241_modes[0];

	imx->regmap = devm_cci_regmap_init_i2c(client, 16);
	if (IS_ERR(imx->regmap))
		return PTR_ERR(imx->regmap);

	ret = imx241_check_hwcfg(imx);
	if (ret)
		return ret;

	mutex_init(&imx->lock);
	v4l2_i2c_subdev_init(&imx->sd, client, &imx241_subdev_ops);
	imx->sd.internal_ops = &imx241_internal_ops;

	imx->xvclk = devm_v4l2_sensor_clk_get(imx->dev, NULL);
	if (IS_ERR(imx->xvclk))
		return dev_err_probe(imx->dev, PTR_ERR(imx->xvclk), "failed to get clock\n");
	rate = clk_get_rate(imx->xvclk);
	if (rate != IMX241_XVCLK_FREQ)
		dev_warn(imx->dev, "clock is %u Hz, expected %u\n", rate, IMX241_XVCLK_FREQ);

	imx->enable = devm_gpiod_get_array_optional(imx->dev, "enable", GPIOD_OUT_LOW);
	if (IS_ERR(imx->enable))
		return dev_err_probe(imx->dev, PTR_ERR(imx->enable), "enable gpios\n");
	imx->reset = devm_gpiod_get_optional(imx->dev, "reset", GPIOD_OUT_HIGH);
	if (IS_ERR(imx->reset))
		return dev_err_probe(imx->dev, PTR_ERR(imx->reset), "reset gpio\n");
	imx->powerdown = devm_gpiod_get_optional(imx->dev, "powerdown", GPIOD_OUT_HIGH);
	if (IS_ERR(imx->powerdown))
		return dev_err_probe(imx->dev, PTR_ERR(imx->powerdown), "powerdown gpio\n");

	for (i = 0; i < IMX241_NUM_SUPPLIES; i++)
		imx->supplies[i].supply = imx241_supply_names[i];
	ret = devm_regulator_bulk_get(imx->dev, IMX241_NUM_SUPPLIES, imx->supplies);
	if (ret)
		return dev_err_probe(imx->dev, ret, "failed to get regulators\n");

	ret = imx241_init_controls(imx);
	if (ret)
		return ret;

	imx->sd.flags |= V4L2_SUBDEV_FL_HAS_DEVNODE;
	imx->sd.entity.function = MEDIA_ENT_F_CAM_SENSOR;
	imx->pad.flags = MEDIA_PAD_FL_SOURCE;
	ret = media_entity_pads_init(&imx->sd.entity, 1, &imx->pad);
	if (ret)
		goto err_ctrls;

	ret = imx241_power_on(imx);
	if (ret)
		goto err_entity;
	ret = imx241_identify(imx);
	if (ret)
		goto err_power;

	pm_runtime_set_active(imx->dev);
	pm_runtime_get_noresume(imx->dev);
	pm_runtime_enable(imx->dev);

	ret = v4l2_async_register_subdev_sensor(&imx->sd);
	if (ret) {
		dev_err(imx->dev, "failed to register subdev: %d\n", ret);
		goto err_pm;
	}

	pm_runtime_set_autosuspend_delay(imx->dev, 1000);
	pm_runtime_use_autosuspend(imx->dev);
	pm_runtime_put_autosuspend(imx->dev);
	return 0;

err_pm:
	pm_runtime_disable(imx->dev);
	pm_runtime_put_noidle(imx->dev);
err_power:
	imx241_power_off(imx);
err_entity:
	media_entity_cleanup(&imx->sd.entity);
err_ctrls:
	v4l2_ctrl_handler_free(&imx->ctrl_handler);
	return ret;
}

static void imx241_remove(struct i2c_client *client)
{
	struct v4l2_subdev *sd = i2c_get_clientdata(client);
	struct imx241 *imx = to_imx241(sd);

	v4l2_async_unregister_subdev(sd);
	media_entity_cleanup(&sd->entity);
	v4l2_ctrl_handler_free(&imx->ctrl_handler);
	pm_runtime_disable(imx->dev);
	if (!pm_runtime_status_suspended(imx->dev))
		imx241_power_off(imx);
	pm_runtime_set_suspended(imx->dev);
	mutex_destroy(&imx->lock);
}

static const struct dev_pm_ops imx241_pm_ops = {
	SET_RUNTIME_PM_OPS(imx241_runtime_suspend, imx241_runtime_resume, NULL)
};

static const struct acpi_device_id imx241_acpi_match[] = {
	{ "INT347F" },
	{ }
};
MODULE_DEVICE_TABLE(acpi, imx241_acpi_match);

static struct i2c_driver imx241_driver = {
	.driver = {
		.name = "imx241",
		.acpi_match_table = imx241_acpi_match,
		.pm = &imx241_pm_ops,
	},
	.probe = imx241_probe,
	.remove = imx241_remove,
};
module_i2c_driver(imx241_driver);

MODULE_DESCRIPTION("Sony IMX241 camera sensor driver");
MODULE_LICENSE("GPL");
