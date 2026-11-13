.PHONY: webui webui-build webui-clean utils utils-arm64 utils-arm32 utils-all utils-clean

webui:
	$(MAKE) -C webui

webui-build:
	$(MAKE) -C webui build

webui-clean:
	$(MAKE) -C webui clean

utils:
	$(MAKE) -C utils

utils-arm64:
	$(MAKE) -C utils arm64

utils-arm32:
	$(MAKE) -C utils arm32

utils-all:
	$(MAKE) -C utils all

utils-clean:
	$(MAKE) -C utils clean