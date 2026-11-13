NDK ?= $(ANDROID_NDK_HOME)
ifeq ($(NDK),)
$(error NDK not found. Set ANDROID_NDK_HOME or pass make NDK=/path/to/android-ndk)
endif

NDK_PREBUILT := $(firstword \
	$(wildcard $(NDK)/toolchains/llvm/prebuilt/linux-x86_64) \
	$(wildcard $(NDK)/toolchains/llvm/prebuilt/linux-aarch64))

ifeq ($(NDK_PREBUILT),)
$(error NDK prebuilt toolchain not found under $(NDK)/toolchains/llvm/prebuilt/)
endif

API ?= 21

CC_ARM64 := $(NDK_PREBUILT)/bin/aarch64-linux-android$(API)-clang
CC_ARM32 := $(NDK_PREBUILT)/bin/armv7-linux-androideabi$(API)-clang
STRIP    := $(NDK_PREBUILT)/bin/llvm-strip

CARGO  := cargo
BINARY := $(shell grep '^name' Cargo.toml | head -1 | sed 's/.*= *"//;s/".*//')
ifeq ($(BINARY),)
BINARY := utils
endif

BUILD_DIR_ARM64 := target/aarch64-linux-android/release
BUILD_DIR_ARM32 := target/armv7-linux-androideabi/release

OUT_ARM64 := outputs/utils-arm64
OUT_ARM32 := outputs/utils-arm32

.PHONY: all arm64 arm32 clean ndk-info

all: arm64 arm32

arm64: $(OUT_ARM64)

arm32: $(OUT_ARM32)

$(OUT_ARM64): $(BUILD_DIR_ARM64)/$(BINARY) | outputs
	$(STRIP) -s $< -o $@
	@echo "output: $@ ($$(ls -lh $@ | awk '{print $$5}'))"

$(OUT_ARM32): $(BUILD_DIR_ARM32)/$(BINARY) | outputs
	$(STRIP) -s $< -o $@
	@echo "output: $@ ($$(ls -lh $@ | awk '{print $$5}'))"

$(BUILD_DIR_ARM64)/$(BINARY):
	CC=$(CC_ARM64) $(CARGO) build --release --target aarch64-linux-android

$(BUILD_DIR_ARM32)/$(BINARY):
	CC=$(CC_ARM32) $(CARGO) build --release --target armv7-linux-androideabi

outputs:
	mkdir -p outputs

clean:
	$(CARGO) clean
	rm -f $(OUT_ARM64) $(OUT_ARM32)

ndk-info:
	@echo "NDK=$(NDK)"
	@echo "PREBUILT=$(NDK_PREBUILT)"
