# Docker Setup for Browser-Use

This directory contains the optimized Docker build system for browser-use, achieving < 30 second builds.

## Quick Start

```bash
# Build base images (only needed once or when dependencies change)
./docker/build-base-images.sh --push

# Build browser-use
# Resolve and record the base-python-deps registry digest after building it.
# BASE_IMAGE must be a published name@sha256:<digest>, not a tag.
docker build -f Dockerfile.fast --build-arg BASE_IMAGE="$BASE_IMAGE" -t browseruse .

# Or use the standard Dockerfile (slower but self-contained)
docker build -t browseruse .
```

## Files

- `Dockerfile` - Standard self-contained build (~2 min)
- `Dockerfile.fast` - Fast build using pre-built base images (~30 sec)
- `docker/` - Base image definitions and build script
  - `base-images/system/` - Python + minimal system deps
  - `base-images/chromium/` - Adds Chromium browser
  - `base-images/python-deps/` - Adds Python dependencies
  - `build-base-images.sh` - Script to build all base images

## Performance

| Build Type | Time |
|------------|------|
| Standard Dockerfile | ~2 minutes |
| Fast build (with base images) | ~30 seconds |
| Rebuild after code change | ~16 seconds |

The fast image checks its tracked `uv.lock` against the checksum embedded in the
base. Rebuild and pin a new base digest when the lock changes. A missing or
mutable base reference fails closed; an old base cannot silently supply a
different dependency graph.
