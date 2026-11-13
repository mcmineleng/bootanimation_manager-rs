# module.mk - 打包脚本

ARCH      ?= arm64
ZIP_LEVEL ?= 1

TEMP_DIR  := temp
WEBROOT   := $(TEMP_DIR)/webroot
MODULE_SRC := module

ifeq ($(ARCH), arm32)
    UTILS_TARGET := arm32
    UTILS_BIN    := utils/outputs/utils-arm32
else
    UTILS_TARGET := arm64
    UTILS_BIN    := utils/outputs/utils-arm64
endif

PKG_NAME  := package-$(ARCH)
ZIP       := $(PKG_NAME).zip

all: package

.PHONY: all package prepare copy-module build-webui copy-webroot build-utils copy-utils clean

package: clean prepare copy-module build-webui copy-webroot build-utils copy-utils zip-package
	@echo "============================================"
	@echo " [完成] 包已生成: $(ZIP)"
	@echo "============================================"

prepare:
	@echo ">> 清理并创建 $(TEMP_DIR)/"
	@rm -rf $(TEMP_DIR)
	@mkdir -p $(TEMP_DIR) $(WEBROOT)
	@mkdir -p $(TEMP_DIR)/bootanimations
	@touch $(TEMP_DIR)/bootanimation.txt

copy-module: prepare
	@echo ">> 拷贝 $(MODULE_SRC)/ 到 $(TEMP_DIR)/"
	@if [ -d $(MODULE_SRC) ]; then \
		cp -r $(MODULE_SRC)/. $(TEMP_DIR)/; \
	else \
		echo "   [警告] $(MODULE_SRC)/ 不存在,跳过"; \
	fi

build-webui:
	@echo ">> 构建 webui"
	$(MAKE) -C webui build

copy-webroot: build-webui prepare
	@echo ">> 拷贝 webui/outputs/ -> $(WEBROOT)/"
	@if [ -d webui/outputs ]; then \
		cp -r webui/outputs/. $(WEBROOT)/; \
	else \
		echo "   [警告] webui/outputs/ 不存在,跳过"; \
	fi

build-utils:
	@echo ">> 编译 utils ($(UTILS_TARGET))"
	$(MAKE) -C utils $(UTILS_TARGET)

copy-utils: build-utils prepare
	@echo ">> 拷贝 $(UTILS_BIN) -> $(TEMP_DIR)/utils"
	@if [ -f $(UTILS_BIN) ]; then \
		cp $(UTILS_BIN) $(TEMP_DIR)/utils; \
		chmod +x $(TEMP_DIR)/utils 2>/dev/null || true; \
	else \
		echo "   [错误] $(UTILS_BIN) 不存在!"; \
		exit 1; \
	fi

zip-package:
	@echo ">> 打包 $(ZIP) (压缩等级: $(ZIP_LEVEL))"
	@rm -f $(ZIP)
	@cd $(TEMP_DIR) && zip -$(ZIP_LEVEL) -r ../$(ZIP) .

clean:
	@echo ">> 清理临时文件"
	@rm -rf $(TEMP_DIR)
	@rm -f $(PKG_NAME).zip
