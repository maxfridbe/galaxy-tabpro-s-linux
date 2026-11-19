# Build the Galaxy TabPro S GNSS module (bcm4752) out of tree.
#   make                      # for the running kernel
#   make KVER=7.3.4-300.fc45.x86_64   # for another installed kernel-devel
#   make sign MOK_KEY=... MOK_CERT=...   # sign for Secure Boot (MOK)
KVER ?= $(shell uname -r)
KDIR ?= /lib/modules/$(KVER)/build
ifeq ($(wildcard $(KDIR)/Makefile),)
KDIR := /usr/src/kernels/$(KVER)
endif
SUBDIRS := bcm4752
MODULES := bcm4752/bcm4752.ko

all:
	@for d in $(SUBDIRS); do $(MAKE) -C $(KDIR) M=$(CURDIR)/$$d modules || exit 1; done

sign: all
	@test -n "$(MOK_KEY)" -a -n "$(MOK_CERT)" || { echo "set MOK_KEY= and MOK_CERT="; exit 1; }
	@for m in $(MODULES); do $(KDIR)/scripts/sign-file sha256 $(MOK_KEY) $(MOK_CERT) $$m && echo "signed $$m"; done

clean:
	@for d in $(SUBDIRS); do $(MAKE) -C $(KDIR) M=$(CURDIR)/$$d clean; done

.PHONY: all sign clean
