// SPDX-License-Identifier: GPL-2.0
/* w700acpi: dump camera-related ACPI method results on the Galaxy TabPro S. Load, read dmesg, unload. */
#include <linux/module.h>
#include <linux/acpi.h>
#include <linux/uuid.h>

static void dump_obj(const char *path, const char *what, union acpi_object *o, int depth)
{
	int i;
	if (!o) { pr_info("w700acpi: %s %s = (null)\n", path, what); return; }
	switch (o->type) {
	case ACPI_TYPE_INTEGER:
		pr_info("w700acpi: %s %s int 0x%llx\n", path, what, o->integer.value); break;
	case ACPI_TYPE_STRING:
		pr_info("w700acpi: %s %s str \"%s\"\n", path, what, o->string.pointer); break;
	case ACPI_TYPE_BUFFER:
		pr_info("w700acpi: %s %s buf len=%u\n", path, what, o->buffer.length);
		print_hex_dump(KERN_INFO, "w700acpi:   ", DUMP_PREFIX_OFFSET, 16, 1, o->buffer.pointer, o->buffer.length, false);
		break;
	case ACPI_TYPE_PACKAGE:
		pr_info("w700acpi: %s %s pkg count=%u\n", path, what, o->package.count);
		if (depth < 3)
			for (i = 0; i < o->package.count; i++) dump_obj(path, what, &o->package.elements[i], depth + 1);
		break;
	case ACPI_TYPE_LOCAL_REFERENCE:
		pr_info("w700acpi: %s %s ref\n", path, what); break;
	default:
		pr_info("w700acpi: %s %s type %d\n", path, what, o->type);
	}
}

static void eval(const char *dev, const char *m)
{
	acpi_handle h;
	struct acpi_buffer out = { ACPI_ALLOCATE_BUFFER, NULL };
	char full[64];
	snprintf(full, sizeof(full), "%s.%s", dev, m);
	if (ACPI_FAILURE(acpi_get_handle(NULL, (acpi_string)full, &h))) { pr_info("w700acpi: %s missing\n", full); return; }
	if (ACPI_FAILURE(acpi_evaluate_object(h, NULL, NULL, &out))) { pr_info("w700acpi: %s eval failed\n", full); return; }
	dump_obj(dev, m, out.pointer, 0);
	kfree(out.pointer);
}

static void dsm(const char *dev, const char *uuid_str, int rev, int maxfn)
{
	acpi_handle h; guid_t g; int fn; char what[48];
	if (ACPI_FAILURE(acpi_get_handle(NULL, (acpi_string)dev, &h))) return;
	if (guid_parse(uuid_str, &g)) return;
	for (fn = 0; fn <= maxfn; fn++) {
		union acpi_object *o = acpi_evaluate_dsm(h, &g, rev, fn, NULL);
		snprintf(what, sizeof(what), "_DSM(%.8s,r%d,f%d)", uuid_str, rev, fn);
		dump_obj(dev, what, o, 0);
		ACPI_FREE(o);
	}
}

static int __init w700acpi_init(void)
{
	static const char * const devs[] = { "\\_SB.PCI0.LNK0", "\\_SB.PCI0.LNK1", "\\_SB.PCI0.CLP0" };
	static const char * const meths[] = { "_HID", "_CID", "_SUB", "_DDN", "_STA", "SSDB", "_CRS", "_PLD", "CLDB", "_DEP" };
	static const char * const uuids[] = {
		"822ace8f-2814-4174-a56b-5f029fe079ee", "5815c5c8-c47d-477b-9a8d-76173176414b",
		"26257549-9271-4ca4-bb43-c4899d5a4881", "79234640-9e10-4fea-a5c1-b5aa8b19756f" };
	int d, m, u;
	for (d = 0; d < ARRAY_SIZE(devs); d++) {
		for (m = 0; m < ARRAY_SIZE(meths); m++) eval(devs[d], meths[m]);
		for (u = 0; u < ARRAY_SIZE(uuids); u++) { dsm(devs[d], uuids[u], 0, 12); dsm(devs[d], uuids[u], 1, 3); }
	}
	return 0;
}
static void __exit w700acpi_exit(void) {}
module_init(w700acpi_init);
module_exit(w700acpi_exit);
MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("Dump camera ACPI data on Samsung Galaxy TabPro S");
