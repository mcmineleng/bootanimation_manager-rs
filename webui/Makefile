PARCEL := npx parcel
DIST   := outputs

.PHONY: build clean

build:
	$(PARCEL) build src/index.html --dist-dir $(DIST) --public-url ./

clean:
	rm -rf .parcel-cache $(DIST)