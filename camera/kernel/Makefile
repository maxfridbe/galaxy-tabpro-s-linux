# Build all Galaxy TabPro S camera modules out of tree.
#   make                      # for the running kernel
#   make KVER=7.3.4-300.fc45  # for another installed kernel-devel
#   make sign MOK_KEY=... MOK_CERT=...   # sign for Secure Boot (MOK)
KVER ?= $(shell uname -r)
KDIR ?= /lib/modules/$(KVER)/build
ifeq ($(wildcard $(KDIR)/Makefile),)
KDIR := /usr/src/kernels/$(KVER)
endif
SUBDIRS := int3472 ipu-bridge imx241 dw9806b
MODULES := int3472/intel_skl_int3472_tps68470.ko ipu-bridge/ipu-bridge.ko imx241/imx241.ko dw9806b/dw9806b.ko

all:
	@for d in $(SUBDIRS); do $(MAKE) -C $(KDIR) M=$(CURDIR)/$$d modules || exit 1; done

sign: all
	@test -n "$(MOK_KEY)" -a -n "$(MOK_CERT)" || { echo "set MOK_KEY= and MOK_CERT="; exit 1; }
	@for m in $(MODULES); do $(KDIR)/scripts/sign-file sha256 $(MOK_KEY) $(MOK_CERT) $$m && echo "signed $$m"; done

clean:
	@for d in $(SUBDIRS); do $(MAKE) -C $(KDIR) M=$(CURDIR)/$$d clean; done

.PHONY: all sign clean
