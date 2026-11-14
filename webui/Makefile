NPM_RUN := npm run
DIST   := outputs

.PHONY: build clean

build:
	$(NPM_RUN) build
	rm outputs/*.map

clean:
	rm -rf .parcel-cache $(DIST)
