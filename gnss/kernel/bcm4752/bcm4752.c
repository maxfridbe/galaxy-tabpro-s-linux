// SPDX-License-Identifier: GPL-2.0
/*
 * Broadcom BCM4752 GNSS receiver on a serdev UART (Samsung Galaxy TabPro S, ACPI SSE4752).
 *
 * Raw pass-through: /dev/gnssN carries the chip's binary protocol, which needs Broadcom's
 * host-side engine (GLL/glgps). The engine changes the UART speed at runtime and drives the
 * power-enable pin itself, so both are exposed as attributes of the serdev device:
 *
 *   speed   UART baud rate (default 115200 from ACPI); applied immediately while open
 *   regpu   REGPU (power enable) GPIO, 0/1; also driven low on close
 */
#include <linux/acpi.h>
#include <linux/gnss.h>
#include <linux/gpio/consumer.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/serdev.h>

struct bcm4752 {
	struct gnss_device *gdev;
	struct serdev_device *serdev;
	struct gpio_desc *regpu;
	struct mutex lock;		/* speed / open state */
	unsigned int speed;
	unsigned int actual;	/* rate the UART really runs at */
	bool open;
};

static int bcm4752_open(struct gnss_device *gdev)
{
	struct bcm4752 *b = gnss_get_drvdata(gdev);
	int ret;

	ret = serdev_device_open(b->serdev);
	if (ret)
		return ret;
	mutex_lock(&b->lock);
	b->actual = serdev_device_set_baudrate(b->serdev, b->speed);
	serdev_device_set_flow_control(b->serdev, true);
	b->open = true;
	mutex_unlock(&b->lock);
	return 0;
}

static void bcm4752_close(struct gnss_device *gdev)
{
	struct bcm4752 *b = gnss_get_drvdata(gdev);

	mutex_lock(&b->lock);
	b->open = false;
	mutex_unlock(&b->lock);
	serdev_device_close(b->serdev);
	gpiod_set_value_cansleep(b->regpu, 0);
}

static int bcm4752_write_raw(struct gnss_device *gdev, const unsigned char *buf, size_t count)
{
	struct bcm4752 *b = gnss_get_drvdata(gdev);
	int ret;

	ret = serdev_device_write(b->serdev, buf, count, MAX_SCHEDULE_TIMEOUT);
	if (ret < 0 || ret < count)
		return ret;
	serdev_device_wait_until_sent(b->serdev, 0);
	return count;
}

static const struct gnss_operations bcm4752_gnss_ops = {
	.open		= bcm4752_open,
	.close		= bcm4752_close,
	.write_raw	= bcm4752_write_raw,
};

static size_t bcm4752_receive_buf(struct serdev_device *serdev, const u8 *buf, size_t count)
{
	struct bcm4752 *b = serdev_device_get_drvdata(serdev);

	return gnss_insert_raw(b->gdev, buf, count);
}

static const struct serdev_device_ops bcm4752_serdev_ops = {
	.receive_buf	= bcm4752_receive_buf,
	.write_wakeup	= serdev_device_write_wakeup,
};

static ssize_t speed_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct bcm4752 *b = dev_get_drvdata(dev);

	return sysfs_emit(buf, "%u\n", b->open ? b->actual : b->speed);
}

static ssize_t speed_store(struct device *dev, struct device_attribute *attr,
			   const char *buf, size_t count)
{
	struct bcm4752 *b = dev_get_drvdata(dev);
	unsigned int v;
	int ret = kstrtouint(buf, 0, &v);

	if (ret)
		return ret;
	if (v < 1200 || v > 4000000)
		return -EINVAL;
	mutex_lock(&b->lock);
	b->speed = v;
	if (b->open) {
		serdev_device_wait_until_sent(b->serdev, msecs_to_jiffies(100));
		b->actual = serdev_device_set_baudrate(b->serdev, v);
		if (b->actual != v)
			dev_warn(dev, "requested %u baud, UART runs at %u\n", v, b->actual);
	}
	mutex_unlock(&b->lock);
	return count;
}
static DEVICE_ATTR_RW(speed);

static ssize_t regpu_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct bcm4752 *b = dev_get_drvdata(dev);

	return sysfs_emit(buf, "%d\n", gpiod_get_value_cansleep(b->regpu));
}

static ssize_t regpu_store(struct device *dev, struct device_attribute *attr,
			   const char *buf, size_t count)
{
	struct bcm4752 *b = dev_get_drvdata(dev);
	bool on;
	int ret = kstrtobool(buf, &on);

	if (ret)
		return ret;
	gpiod_set_value_cansleep(b->regpu, on);
	return count;
}
static DEVICE_ATTR_RW(regpu);

static struct attribute *bcm4752_attrs[] = {
	&dev_attr_speed.attr,
	&dev_attr_regpu.attr,
	NULL
};
ATTRIBUTE_GROUPS(bcm4752);

static const struct acpi_gpio_params regpu_gpio = { 0, 0, false };
static const struct acpi_gpio_mapping bcm4752_acpi_gpios[] = {
	{ "regpu-gpios", &regpu_gpio, 1 },
	{ }
};

static int bcm4752_probe(struct serdev_device *serdev)
{
	struct device *dev = &serdev->dev;
	struct gnss_device *gdev;
	struct bcm4752 *b;
	int ret;

	b = devm_kzalloc(dev, sizeof(*b), GFP_KERNEL);
	if (!b)
		return -ENOMEM;
	b->serdev = serdev;
	b->speed = 115200;
	mutex_init(&b->lock);
	device_property_read_u32(dev, "current-speed", &b->speed);

	if (has_acpi_companion(dev)) {
		ret = devm_acpi_dev_add_driver_gpios(dev, bcm4752_acpi_gpios);
		if (ret)
			return ret;
	}
	b->regpu = devm_gpiod_get(dev, "regpu", GPIOD_OUT_LOW);
	if (IS_ERR(b->regpu))
		return dev_err_probe(dev, PTR_ERR(b->regpu), "no REGPU gpio\n");

	gdev = gnss_allocate_device(dev);
	if (!gdev)
		return -ENOMEM;
	gdev->type = GNSS_TYPE_NMEA;	/* closest type; the stream is Broadcom binary */
	gdev->ops = &bcm4752_gnss_ops;
	gnss_set_drvdata(gdev, b);
	b->gdev = gdev;

	serdev_device_set_drvdata(serdev, b);
	serdev_device_set_client_ops(serdev, &bcm4752_serdev_ops);

	ret = gnss_register_device(gdev);
	if (ret) {
		gnss_put_device(gdev);
		return ret;
	}
	return 0;
}

static void bcm4752_remove(struct serdev_device *serdev)
{
	struct bcm4752 *b = serdev_device_get_drvdata(serdev);

	gnss_deregister_device(b->gdev);
	gnss_put_device(b->gdev);
}

static const struct acpi_device_id bcm4752_acpi_match[] = {
	{ "SSE4752" },		/* Samsung Galaxy TabPro S */
	{ }
};
MODULE_DEVICE_TABLE(acpi, bcm4752_acpi_match);

static struct serdev_device_driver bcm4752_driver = {
	.driver = {
		.name		= "gnss-bcm4752",
		.acpi_match_table = bcm4752_acpi_match,
		.dev_groups	= bcm4752_groups,
	},
	.probe	= bcm4752_probe,
	.remove	= bcm4752_remove,
};
module_serdev_device_driver(bcm4752_driver);

MODULE_DESCRIPTION("Broadcom BCM4752 GNSS receiver (raw serdev pass-through)");
MODULE_LICENSE("GPL");
