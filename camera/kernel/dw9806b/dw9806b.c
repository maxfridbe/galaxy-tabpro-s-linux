// SPDX-License-Identifier: GPL-2.0
// Copyright (C) 2018 Intel Corporation

#include <linux/acpi.h>
#include <linux/delay.h>
#include <linux/i2c.h>
#include <linux/iopoll.h>
#include <linux/module.h>
#include <linux/pm_runtime.h>
#include <linux/regulator/consumer.h>
#include <media/v4l2-ctrls.h>
#include <media/v4l2-device.h>

#define DW9806B_MAX_FOCUS_POS	1023
/*
 * This sets the minimum granularity for the focus positions.
 * A value of 1 gives maximum accuracy for a desired focus position.
 */
#define DW9806B_FOCUS_STEPS	1
/*
 * This acts as the minimum granularity of lens movement.
 * Keep this value power of 2, so the control steps can be
 * uniformly adjusted for gradual lens movement, with desired
 * number of control steps.
 */
#define DW9806B_CTRL_STEPS	16
#define DW9806B_CTRL_DELAY_US	1000

#define DW9806B_CTL_ADDR		0x02
/*
 * DW9806B separates two registers to control the VCM position.
 * One for MSB value, another is LSB value.
 */
#define DW9806B_MSB_ADDR		0x03
#define DW9806B_LSB_ADDR		0x04
#define DW9806B_STATUS_ADDR	0x05
#define DW9806B_MODE_ADDR	0x06
#define DW9806B_RESONANCE_ADDR	0x07

#define MAX_RETRY		10

static const char * const dw9806b_supply_names[] = { "vdd", "vsio" };

struct dw9806b_device {
	struct regulator_bulk_data supplies[ARRAY_SIZE(dw9806b_supply_names)];
	struct v4l2_ctrl_handler ctrls_vcm;
	struct v4l2_subdev sd;
	u16 current_val;
};

static inline struct dw9806b_device *sd_to_dw9806b_vcm(
					struct v4l2_subdev *subdev)
{
	return container_of(subdev, struct dw9806b_device, sd);
}

static int dw9806b_i2c_check(struct i2c_client *client)
{
	const char status_addr = DW9806B_STATUS_ADDR;
	char status_result;
	int ret;

	ret = i2c_master_send(client, &status_addr, sizeof(status_addr));
	if (ret < 0) {
		dev_err(&client->dev, "I2C write STATUS address fail ret = %d\n",
			ret);
		return ret;
	}

	ret = i2c_master_recv(client, &status_result, sizeof(status_result));
	if (ret < 0) {
		dev_err(&client->dev, "I2C read STATUS value fail ret = %d\n",
			ret);
		return ret;
	}

	return status_result;
}

static int dw9806b_set_dac(struct i2c_client *client, u16 data)
{
	const char tx_data[3] = {
		DW9806B_MSB_ADDR, ((data >> 8) & 0x03), (data & 0xff)
	};
	int val, ret;

	/*
	 * According to the datasheet, need to check the bus status before we
	 * write VCM position. This ensure that we really write the value
	 * into the register
	 */
	ret = readx_poll_timeout(dw9806b_i2c_check, client, val, val <= 0,
			DW9806B_CTRL_DELAY_US, MAX_RETRY * DW9806B_CTRL_DELAY_US);

	if (ret || val < 0) {
		if (ret) {
			dev_warn(&client->dev,
				"Cannot do the write operation because VCM is busy\n");
		}

		return ret ? -EBUSY : val;
	}

	/* Write VCM position to registers */
	ret = i2c_master_send(client, tx_data, sizeof(tx_data));
	if (ret < 0) {
		dev_err(&client->dev,
			"I2C write MSB fail ret=%d\n", ret);

		return ret;
	}

	return 0;
}

static int dw9806b_set_ctrl(struct v4l2_ctrl *ctrl)
{
	struct dw9806b_device *dev_vcm = container_of(ctrl->handler,
		struct dw9806b_device, ctrls_vcm);

	if (ctrl->id == V4L2_CID_FOCUS_ABSOLUTE) {
		struct i2c_client *client = v4l2_get_subdevdata(&dev_vcm->sd);

		dev_vcm->current_val = ctrl->val;
		return dw9806b_set_dac(client, ctrl->val);
	}

	return -EINVAL;
}

static const struct v4l2_ctrl_ops dw9806b_vcm_ctrl_ops = {
	.s_ctrl = dw9806b_set_ctrl,
};

static int dw9806b_open(struct v4l2_subdev *sd, struct v4l2_subdev_fh *fh)
{
	return pm_runtime_resume_and_get(sd->dev);
}

static int dw9806b_close(struct v4l2_subdev *sd, struct v4l2_subdev_fh *fh)
{
	pm_runtime_put(sd->dev);

	return 0;
}

static const struct v4l2_subdev_internal_ops dw9806b_int_ops = {
	.open = dw9806b_open,
	.close = dw9806b_close,
};

static const struct v4l2_subdev_ops dw9806b_ops = { };

static void dw9806b_subdev_cleanup(struct dw9806b_device *dw9806b_dev)
{
	v4l2_async_unregister_subdev(&dw9806b_dev->sd);
	v4l2_ctrl_handler_free(&dw9806b_dev->ctrls_vcm);
	media_entity_cleanup(&dw9806b_dev->sd.entity);
}

static int dw9806b_init_controls(struct dw9806b_device *dev_vcm)
{
	struct v4l2_ctrl_handler *hdl = &dev_vcm->ctrls_vcm;
	const struct v4l2_ctrl_ops *ops = &dw9806b_vcm_ctrl_ops;
	struct i2c_client *client = v4l2_get_subdevdata(&dev_vcm->sd);

	v4l2_ctrl_handler_init(hdl, 1);

	v4l2_ctrl_new_std(hdl, ops, V4L2_CID_FOCUS_ABSOLUTE,
			  0, DW9806B_MAX_FOCUS_POS, DW9806B_FOCUS_STEPS, 0);

	dev_vcm->sd.ctrl_handler = hdl;
	if (hdl->error) {
		dev_err(&client->dev, "%s fail error: 0x%x\n",
			__func__, hdl->error);
		return hdl->error;
	}

	return 0;
}

static int dw9806b_probe(struct i2c_client *client)
{
	struct dw9806b_device *dw9806b_dev;
	int rval;

	dw9806b_dev = devm_kzalloc(&client->dev, sizeof(*dw9806b_dev),
				  GFP_KERNEL);
	if (dw9806b_dev == NULL)
		return -ENOMEM;

	for (rval = 0; rval < ARRAY_SIZE(dw9806b_supply_names); rval++)
		dw9806b_dev->supplies[rval].supply = dw9806b_supply_names[rval];
	rval = devm_regulator_bulk_get(&client->dev, ARRAY_SIZE(dw9806b_supply_names),
				       dw9806b_dev->supplies);
	if (rval)
		return dev_err_probe(&client->dev, rval, "failed to get regulators\n");

	v4l2_i2c_subdev_init(&dw9806b_dev->sd, client, &dw9806b_ops);
	dw9806b_dev->sd.flags |= V4L2_SUBDEV_FL_HAS_DEVNODE;
	dw9806b_dev->sd.internal_ops = &dw9806b_int_ops;

	rval = dw9806b_init_controls(dw9806b_dev);
	if (rval)
		goto err_cleanup;

	rval = media_entity_pads_init(&dw9806b_dev->sd.entity, 0, NULL);
	if (rval < 0)
		goto err_cleanup;

	dw9806b_dev->sd.entity.function = MEDIA_ENT_F_LENS;

	rval = v4l2_async_register_subdev(&dw9806b_dev->sd);
	if (rval < 0)
		goto err_cleanup;

	pm_runtime_enable(&client->dev);

	return 0;

err_cleanup:
	v4l2_ctrl_handler_free(&dw9806b_dev->ctrls_vcm);
	media_entity_cleanup(&dw9806b_dev->sd.entity);

	return rval;
}

static void dw9806b_remove(struct i2c_client *client)
{
	struct v4l2_subdev *sd = i2c_get_clientdata(client);
	struct dw9806b_device *dw9806b_dev = sd_to_dw9806b_vcm(sd);

	pm_runtime_disable(&client->dev);

	dw9806b_subdev_cleanup(dw9806b_dev);
}

/*
 * This function sets the vcm position, so it consumes least current
 * The lens position is gradually moved in units of DW9806B_CTRL_STEPS,
 * to make the movements smoothly.
 */
static int __maybe_unused dw9806b_vcm_suspend(struct device *dev)
{
	struct i2c_client *client = to_i2c_client(dev);
	struct v4l2_subdev *sd = i2c_get_clientdata(client);
	struct dw9806b_device *dw9806b_dev = sd_to_dw9806b_vcm(sd);
	const char tx_data[2] = { DW9806B_CTL_ADDR, 0x01 };
	int ret, val;

	for (val = dw9806b_dev->current_val & ~(DW9806B_CTRL_STEPS - 1);
	     val >= 0; val -= DW9806B_CTRL_STEPS) {
		ret = dw9806b_set_dac(client, val);
		if (ret)
			dev_err_once(dev, "%s I2C failure: %d", __func__, ret);
		usleep_range(DW9806B_CTRL_DELAY_US, DW9806B_CTRL_DELAY_US + 10);
	}

	/* Power down */
	ret = i2c_master_send(client, tx_data, sizeof(tx_data));
	if (ret < 0)
		dev_err(&client->dev, "I2C write CTL fail ret = %d\n", ret);

	regulator_bulk_disable(ARRAY_SIZE(dw9806b_dev->supplies), dw9806b_dev->supplies);
	return 0;
}

/*
 * This function sets the vcm position to the value set by the user
 * through v4l2_ctrl_ops s_ctrl handler
 * The lens position is gradually moved in units of DW9806B_CTRL_STEPS,
 * to make the movements smoothly.
 */
static int  __maybe_unused dw9806b_vcm_resume(struct device *dev)
{
	struct i2c_client *client = to_i2c_client(dev);
	struct v4l2_subdev *sd = i2c_get_clientdata(client);
	struct dw9806b_device *dw9806b_dev = sd_to_dw9806b_vcm(sd);
	/* Init sequence from the Windows imx241.sys DW9806B_Init table */
	static const u8 init[][2] = {
		{ DW9806B_CTL_ADDR, 0x01 }, { DW9806B_CTL_ADDR, 0x00 },
		{ 0xff, 1 },	/* delay 1 ms */
		{ DW9806B_CTL_ADDR, 0x02 }, { DW9806B_MODE_ADDR, 0x61 },
		{ DW9806B_RESONANCE_ADDR, 0x13 },
	};
	unsigned int i;
	int ret, val;

	ret = regulator_bulk_enable(ARRAY_SIZE(dw9806b_dev->supplies), dw9806b_dev->supplies);
	if (ret)
		return ret;
	usleep_range(2000, 3000);

	for (i = 0; i < ARRAY_SIZE(init); i++) {
		if (init[i][0] == 0xff) {
			usleep_range(init[i][1] * 1000, init[i][1] * 1000 + 100);
			continue;
		}
		ret = i2c_master_send(client, init[i], 2);
		if (ret < 0) {
			dev_err(&client->dev, "init write 0x%02x fail %d\n", init[i][0], ret);
			regulator_bulk_disable(ARRAY_SIZE(dw9806b_dev->supplies), dw9806b_dev->supplies);
			return ret;
		}
	}

	for (val = dw9806b_dev->current_val % DW9806B_CTRL_STEPS;
	     val < dw9806b_dev->current_val + DW9806B_CTRL_STEPS - 1;
	     val += DW9806B_CTRL_STEPS) {
		ret = dw9806b_set_dac(client, val);
		if (ret)
			dev_err_ratelimited(dev, "%s I2C failure: %d",
						__func__, ret);
		usleep_range(DW9806B_CTRL_DELAY_US, DW9806B_CTRL_DELAY_US + 10);
	}

	return 0;
}

static const struct of_device_id dw9806b_of_table[] = {
	{ .compatible = "dongwoon,dw9806b-vcm" },
	/* Compatibility for older firmware, NEVER USE THIS IN FIRMWARE! */
	{ .compatible = "dongwoon,dw9806b" },
	{ /* sentinel */ }
};
MODULE_DEVICE_TABLE(of, dw9806b_of_table);

static const struct i2c_device_id dw9806b_id_table[] = {
	{ "dw9806b" },
	{ }
};
MODULE_DEVICE_TABLE(i2c, dw9806b_id_table);

static const struct dev_pm_ops dw9806b_pm_ops = {
	SET_SYSTEM_SLEEP_PM_OPS(dw9806b_vcm_suspend, dw9806b_vcm_resume)
	SET_RUNTIME_PM_OPS(dw9806b_vcm_suspend, dw9806b_vcm_resume, NULL)
};

static struct i2c_driver dw9806b_i2c_driver = {
	.driver = {
		.name = "dw9806b",
		.pm = &dw9806b_pm_ops,
		.of_match_table = dw9806b_of_table,
	},
	.id_table = dw9806b_id_table,
	.probe = dw9806b_probe,
	.remove = dw9806b_remove,
};

module_i2c_driver(dw9806b_i2c_driver);

MODULE_AUTHOR("Chiang, Alan");
MODULE_DESCRIPTION("DW9806B VCM driver (derived from dw9807-vcm)");
MODULE_LICENSE("GPL v2");
