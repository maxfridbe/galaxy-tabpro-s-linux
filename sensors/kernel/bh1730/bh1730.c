// SPDX-License-Identifier: GPL-2.0
/*
 * ROHM BH1730FVC ambient light sensor (Samsung Galaxy TabPro S, ACPI TOBH1733).
 *
 * Continuous measurement, automatic gain (x1/x2/x64/x128), lux per the datasheet formula.
 * Exposes in_illuminance_input (lux) and the raw visible+IR / IR channels.
 */
#include <linux/acpi.h>
#include <linux/delay.h>
#include <linux/i2c.h>
#include <linux/iio/iio.h>
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

#define BH1730_ITIME			0xda	/* 38 cycles: 102.6 ms */
#define BH1730_ITIME_US			(2699 * (256 - BH1730_ITIME))
#define BH1730_SATURATION		60000
#define BH1730_TOO_DARK			200

static const int bh1730_gains[] = { 1, 2, 64, 128 };	/* GAIN register values 0..3 */

struct bh1730 {
	struct i2c_client *client;
	struct mutex lock;	/* serialises measurements and gain changes */
	int gain_idx;
};

static int bh1730_write(struct bh1730 *b, u8 reg, u8 val)
{
	return i2c_smbus_write_byte_data(b->client, BH1730_CMD | reg, val);
}

static int bh1730_read(struct bh1730 *b, u8 reg)
{
	return i2c_smbus_read_byte_data(b->client, BH1730_CMD | reg);
}

static int bh1730_set_gain(struct bh1730 *b, int idx)
{
	int ret = bh1730_write(b, BH1730_REG_GAIN, idx);

	if (!ret)
		b->gain_idx = idx;
	return ret;
}

/* Wait for a fresh conversion (one full integration period) and read both channels. */
static int bh1730_measure(struct bh1730 *b, int *d0, int *d1)
{
	int ret, i;

	/* discard the conversion started with the old settings */
	usleep_range(BH1730_ITIME_US + 5000, BH1730_ITIME_US + 10000);
	for (i = 0; i < 20; i++) {
		ret = bh1730_read(b, BH1730_REG_CONTROL);
		if (ret < 0)
			return ret;
		if (ret & BH1730_CTRL_ADC_VALID)
			break;
		usleep_range(10000, 12000);
	}
	ret = i2c_smbus_read_word_data(b->client, BH1730_CMD | BH1730_REG_DATA0);
	if (ret < 0)
		return ret;
	*d0 = ret;
	ret = i2c_smbus_read_word_data(b->client, BH1730_CMD | BH1730_REG_DATA1);
	if (ret < 0)
		return ret;
	*d1 = ret;
	return 0;
}

/* Measure with automatic gain selection. */
static int bh1730_measure_auto(struct bh1730 *b, int *d0, int *d1)
{
	int ret, tries;

	for (tries = 0; tries < 4; tries++) {
		ret = bh1730_measure(b, d0, d1);
		if (ret)
			return ret;
		if ((*d0 >= BH1730_SATURATION || *d1 >= BH1730_SATURATION) && b->gain_idx > 0)
			ret = bh1730_set_gain(b, b->gain_idx - 1);
		else if (*d0 < BH1730_TOO_DARK && b->gain_idx < ARRAY_SIZE(bh1730_gains) - 1)
			ret = bh1730_set_gain(b, b->gain_idx + 1);
		else
			return 0;
		if (ret)
			return ret;
	}
	return 0;
}

/* Datasheet lux formula, result in milli-lux. */
static s64 bh1730_mlux(int d0, int d1, int gain)
{
	s64 c0, c1, v;

	if (!d0)
		return 0;
	if (1000LL * d1 < 260LL * d0) {
		c0 = 1290; c1 = 2733;
	} else if (1000LL * d1 < 550LL * d0) {
		c0 = 795; c1 = 859;
	} else if (1000LL * d1 < 1090LL * d0) {
		c0 = 510; c1 = 345;
	} else if (1000LL * d1 < 2130LL * d0) {
		c0 = 276; c1 = 130;
	} else {
		return 0;
	}
	v = c0 * d0 - c1 * d1;		/* x1000 */
	if (v < 0)
		return 0;
	/* lx = v/1000 / gain * 102.6 ms / itime  ->  mlx = v * 102600 / (gain * itime_us) */
	return div64_s64(v * 102600, (s64)gain * BH1730_ITIME_US);
}

static int bh1730_read_raw(struct iio_dev *indio_dev, struct iio_chan_spec const *chan,
			   int *val, int *val2, long mask)
{
	struct bh1730 *b = iio_priv(indio_dev);
	int d0, d1, ret;
	s64 mlux;

	mutex_lock(&b->lock);
	ret = bh1730_measure_auto(b, &d0, &d1);
	mutex_unlock(&b->lock);
	if (ret)
		return ret;

	switch (mask) {
	case IIO_CHAN_INFO_PROCESSED:
		mlux = bh1730_mlux(d0, d1, bh1730_gains[b->gain_idx]);
		*val = div_s64(mlux, 1000);
		*val2 = (mlux - (s64)*val * 1000) * 1000;
		return IIO_VAL_INT_PLUS_MICRO;
	case IIO_CHAN_INFO_RAW:
		*val = chan->channel2 == IIO_MOD_LIGHT_IR ? d1 : d0;
		return IIO_VAL_INT;
	case IIO_CHAN_INFO_HARDWAREGAIN:
		*val = bh1730_gains[b->gain_idx];
		return IIO_VAL_INT;
	}
	return -EINVAL;
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
		.info_mask_shared_by_type = BIT(IIO_CHAN_INFO_HARDWAREGAIN),
	},
	{
		.type = IIO_INTENSITY,
		.modified = 1,
		.channel2 = IIO_MOD_LIGHT_IR,
		.info_mask_separate = BIT(IIO_CHAN_INFO_RAW),
		.info_mask_shared_by_type = BIT(IIO_CHAN_INFO_HARDWAREGAIN),
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

	ret = bh1730_write(b, BH1730_REG_TIMING, BH1730_ITIME);
	if (!ret)
		ret = bh1730_set_gain(b, 2);	/* start at x64, auto-ranged per read */
	if (!ret)
		ret = bh1730_write(b, BH1730_REG_CONTROL, BH1730_CTRL_POWER | BH1730_CTRL_ADC_EN);
	if (ret)
		return dev_err_probe(&client->dev, ret, "cannot configure sensor\n");
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
