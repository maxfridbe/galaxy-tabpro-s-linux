// SPDX-License-Identifier: GPL-2.0
/*
 * ROHM BH1730FVC ambient light sensor (Samsung Galaxy TabPro S, ACPI TOBH1733).
 *
 * The sensor measures continuously; reads return the latest completed conversion without
 * sleeping, so pollers such as iio-sensor-proxy are never blocked. Gain (x1/x2/x64/x128) and
 * integration time (102.6 ms / 688.5 ms) are adjusted automatically: in dim light the long
 * integration gives ~7x more counts and far less jitter.
 *
 * Lux uses the coefficients the TabPro S firmware provides (ACPI _DSM of ALS1, identical to the
 * defaults of ROHM's Windows driver). They compensate for the dark cover glass and are ~3.5x the
 * bare-sensor datasheet values:  lux = (c0*DATA0 - c1*DATA1) * 100 / (gain * t_int[ms])
 */
#include <linux/acpi.h>
#include <linux/delay.h>
#include <linux/i2c.h>
#include <linux/iio/iio.h>
#include <linux/jiffies.h>
#include <linux/module.h>
#include <linux/mod_devicetable.h>

#define BH1730_CMD			0x80
#define BH1730_CMD_SW_RESET		0xe4

#define BH1730_REG_CONTROL		0x00
#define BH1730_REG_TIMING		0x01
#define BH1730_REG_GAIN			0x07
#define BH1730_REG_ID			0x12
#define BH1730_REG_DATA0		0x14
#define BH1730_REG_DATA1		0x16

#define BH1730_CTRL_ADC_VALID		BIT(4)
#define BH1730_CTRL_ADC_EN		BIT(1)
#define BH1730_CTRL_POWER		BIT(0)

#define BH1730_CYCLE_US			2700
#define BH1730_SATURATION		60000
#define BH1730_TOO_DARK			1000	/* counts: step up sensitivity below this */
#define BH1730_TOO_BRIGHT_LONG		30000	/* counts: leave long integration above this */

static const int bh1730_gains[] = { 1, 2, 64, 128 };	/* GAIN register values 0..3 */
static u8 bh1730_itimes[] = { 0xda, 0x01 };	/* 38 cycles = 102.6 ms, 255 = 688.5 ms (0x00 reads 0) */
module_param_named(long_itime, bh1730_itimes[1], byte, 0644);
MODULE_PARM_DESC(long_itime, "TIMING register value for dim light");

/* firmware coefficients (x1000): DATA1/DATA0 ratio limit, c0, c1 */
static const struct { int ratio, c0, c1; } bh1730_coef[] = {
	{  558, 4478, 5816 },
	{  697, 3674, 4376 },
	{  777, 1858, 1770 },
	{ 3000,  652,  217 },
};

struct bh1730 {
	struct i2c_client *client;
	struct mutex lock;	/* serialises register access and the cached state */
	int gain_idx, time_idx;
	unsigned long valid_from;	/* jiffies: data with the current settings available */
	bool have_data;
	int d0, d1, data_gain, data_time;	/* last good conversion and its settings */
};

static int bh1730_write(struct bh1730 *b, u8 reg, u8 val)
{
	return i2c_smbus_write_byte_data(b->client, BH1730_CMD | reg, val);
}

static int bh1730_read(struct bh1730 *b, u8 reg)
{
	return i2c_smbus_read_byte_data(b->client, BH1730_CMD | reg);
}

static int bh1730_itime_us(int time_idx)
{
	return BH1730_CYCLE_US * (256 - bh1730_itimes[time_idx]);
}

static int bh1730_apply(struct bh1730 *b, int gain_idx, int time_idx)
{
	int ret = bh1730_write(b, BH1730_REG_GAIN, gain_idx);

	if (!ret)
		ret = bh1730_write(b, BH1730_REG_TIMING, bh1730_itimes[time_idx]);
	if (ret)
		return ret;
	b->gain_idx = gain_idx;
	b->time_idx = time_idx;
	/* the running conversion still uses the old settings: wait for the next full one */
	b->valid_from = jiffies + usecs_to_jiffies(2 * bh1730_itime_us(time_idx) + 10000);
	return 0;
}

/* Pick the next more/less sensitive setting; returns false if already at the limit. */
static bool bh1730_next(struct bh1730 *b, int d0, int d1, int *g, int *t)
{
	*g = b->gain_idx;
	*t = b->time_idx;
	if (d0 >= BH1730_SATURATION || d1 >= BH1730_SATURATION) {
		if (*t > 0)
			(*t)--;
		else if (*g > 0)
			(*g)--;
		else
			return false;
	} else if (*t > 0 && d0 > BH1730_TOO_BRIGHT_LONG) {
		(*t)--;
	} else if (d0 < BH1730_TOO_DARK) {
		if (*g < ARRAY_SIZE(bh1730_gains) - 1)
			(*g)++;
		else if (*t < ARRAY_SIZE(bh1730_itimes) - 1)
			(*t)++;
		else
			return false;
	} else {
		return false;
	}
	return true;
}

/* Fetch the latest conversion if one with the current settings is ready; adjust range. */
static int bh1730_update(struct bh1730 *b)
{
	int ret, d0, d1, g, t;

	if (time_before(jiffies, b->valid_from))
		return 0;
	ret = bh1730_read(b, BH1730_REG_CONTROL);
	if (ret < 0)
		return ret;
	if (!(ret & BH1730_CTRL_ADC_VALID))
		return 0;
	ret = i2c_smbus_read_word_data(b->client, BH1730_CMD | BH1730_REG_DATA0);
	if (ret < 0)
		return ret;
	d0 = ret;
	ret = i2c_smbus_read_word_data(b->client, BH1730_CMD | BH1730_REG_DATA1);
	if (ret < 0)
		return ret;
	d1 = ret;

	/* keep a saturated reading only if nothing better is possible */
	if (bh1730_next(b, d0, d1, &g, &t)) {
		if ((d0 < BH1730_SATURATION && d1 < BH1730_SATURATION) || !b->have_data) {
			b->d0 = d0; b->d1 = d1;
			b->data_gain = bh1730_gains[b->gain_idx];
			b->data_time = bh1730_itime_us(b->time_idx);
			b->have_data = true;
		}
		return bh1730_apply(b, g, t);
	}
	b->d0 = d0; b->d1 = d1;
	b->data_gain = bh1730_gains[b->gain_idx];
	b->data_time = bh1730_itime_us(b->time_idx);
	b->have_data = true;
	return 0;
}

static s64 bh1730_mlux(struct bh1730 *b)
{
	s64 v;
	int i;

	if (!b->d0)
		return 0;
	for (i = 0; i < ARRAY_SIZE(bh1730_coef); i++)
		if (1000LL * b->d1 < (s64)bh1730_coef[i].ratio * b->d0)
			break;
	if (i == ARRAY_SIZE(bh1730_coef))
		return 0;
	v = (s64)bh1730_coef[i].c0 * b->d0 - (s64)bh1730_coef[i].c1 * b->d1;	/* x1000 */
	if (v < 0)
		return 0;
	/* mlx = v/1000 * 100 / (gain * t_ms) * 1000 = v * 100000 / (gain * t_us) */
	return div64_s64(v * 100000, (s64)b->data_gain * b->data_time);
}

static int bh1730_read_raw(struct iio_dev *indio_dev, struct iio_chan_spec const *chan,
			   int *val, int *val2, long mask)
{
	struct bh1730 *b = iio_priv(indio_dev);
	int ret, tries;
	s64 mlux;

	mutex_lock(&b->lock);
	ret = bh1730_update(b);
	/* first read after probe: wait for a conversion once */
	for (tries = 0; !ret && !b->have_data && tries < 10; tries++) {
		msleep(DIV_ROUND_UP(bh1730_itime_us(b->time_idx), 1000) + 5);
		ret = bh1730_update(b);
	}
	if (!ret && !b->have_data)
		ret = -EAGAIN;
	if (ret) {
		mutex_unlock(&b->lock);
		return ret;
	}

	switch (mask) {
	case IIO_CHAN_INFO_PROCESSED:
		mlux = bh1730_mlux(b);
		*val = div_s64(mlux, 1000);
		*val2 = (mlux - (s64)*val * 1000) * 1000;
		ret = IIO_VAL_INT_PLUS_MICRO;
		break;
	case IIO_CHAN_INFO_RAW:
		*val = chan->channel2 == IIO_MOD_LIGHT_IR ? b->d1 : b->d0;
		ret = IIO_VAL_INT;
		break;
	case IIO_CHAN_INFO_HARDWAREGAIN:
		*val = b->data_gain;
		ret = IIO_VAL_INT;
		break;
	case IIO_CHAN_INFO_INT_TIME:
		*val = 0;
		*val2 = b->data_time;
		ret = IIO_VAL_INT_PLUS_MICRO;
		break;
	default:
		ret = -EINVAL;
	}
	mutex_unlock(&b->lock);
	return ret;
}

static const struct iio_info bh1730_info = {
	.read_raw = bh1730_read_raw,
};

static const struct iio_chan_spec bh1730_channels[] = {
	{
		.type = IIO_LIGHT,
		.info_mask_separate = BIT(IIO_CHAN_INFO_PROCESSED),
	},
	{
		.type = IIO_INTENSITY,
		.modified = 1,
		.channel2 = IIO_MOD_LIGHT_BOTH,
		.info_mask_separate = BIT(IIO_CHAN_INFO_RAW),
		.info_mask_shared_by_type = BIT(IIO_CHAN_INFO_HARDWAREGAIN) |
					    BIT(IIO_CHAN_INFO_INT_TIME),
	},
	{
		.type = IIO_INTENSITY,
		.modified = 1,
		.channel2 = IIO_MOD_LIGHT_IR,
		.info_mask_separate = BIT(IIO_CHAN_INFO_RAW),
		.info_mask_shared_by_type = BIT(IIO_CHAN_INFO_HARDWAREGAIN) |
					    BIT(IIO_CHAN_INFO_INT_TIME),
	},
};

static void bh1730_power_off(void *data)
{
	struct bh1730 *b = data;

	bh1730_write(b, BH1730_REG_CONTROL, 0);
}

static int bh1730_probe(struct i2c_client *client)
{
	struct iio_dev *indio_dev;
	struct bh1730 *b;
	int ret;

	indio_dev = devm_iio_device_alloc(&client->dev, sizeof(*b));
	if (!indio_dev)
		return -ENOMEM;
	b = iio_priv(indio_dev);
	b->client = client;
	mutex_init(&b->lock);

	i2c_smbus_write_byte(client, BH1730_CMD_SW_RESET);
	usleep_range(1000, 2000);
	ret = bh1730_read(b, BH1730_REG_ID);
	if (ret < 0)
		return dev_err_probe(&client->dev, ret, "cannot read part id\n");
	if ((ret & 0xf0) != 0x70)
		return dev_err_probe(&client->dev, -ENODEV, "unexpected part id 0x%02x\n", ret);

	ret = bh1730_apply(b, 2, 0);		/* x64, 102.6 ms; auto-ranged from here */
	if (!ret)
		ret = bh1730_write(b, BH1730_REG_CONTROL, BH1730_CTRL_POWER | BH1730_CTRL_ADC_EN);
	if (ret)
		return dev_err_probe(&client->dev, ret, "cannot configure sensor\n");
	b->valid_from = jiffies + usecs_to_jiffies(bh1730_itime_us(0) + 10000);
	ret = devm_add_action_or_reset(&client->dev, bh1730_power_off, b);
	if (ret)
		return ret;

	indio_dev->name = "bh1730";
	indio_dev->info = &bh1730_info;
	indio_dev->modes = INDIO_DIRECT_MODE;
	indio_dev->channels = bh1730_channels;
	indio_dev->num_channels = ARRAY_SIZE(bh1730_channels);
	return devm_iio_device_register(&client->dev, indio_dev);
}

static const struct acpi_device_id bh1730_acpi_match[] = {
	{ "TOBH1733" },		/* Samsung Galaxy TabPro S */
	{ }
};
MODULE_DEVICE_TABLE(acpi, bh1730_acpi_match);

static const struct of_device_id bh1730_of_match[] = {
	{ .compatible = "rohm,bh1730fvc" },
	{ }
};
MODULE_DEVICE_TABLE(of, bh1730_of_match);

static const struct i2c_device_id bh1730_id[] = {
	{ "bh1730" },
	{ }
};
MODULE_DEVICE_TABLE(i2c, bh1730_id);

static struct i2c_driver bh1730_driver = {
	.driver = {
		.name = "bh1730",
		.acpi_match_table = bh1730_acpi_match,
		.of_match_table = bh1730_of_match,
	},
	.probe = bh1730_probe,
	.id_table = bh1730_id,
};
module_i2c_driver(bh1730_driver);

MODULE_DESCRIPTION("ROHM BH1730FVC ambient light sensor driver");
MODULE_LICENSE("GPL");
