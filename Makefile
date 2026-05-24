# Iris — convenience targets.
#
# `make deploy` rebuilds + restarts, stamping the web bundle with the current
# git commit so already-open browser tabs detect the redeploy and offer a
# reload (see web/src/components/UpdateBanner.tsx). `.git` is excluded from the
# Docker build context, so Vite CANNOT read the sha inside the build — it must
# be injected from the host. That's the whole point of this target.
#
# Pass extra compose flags via ARGS, e.g.:
#   make deploy ARGS="--profile cloudflared"
#
# Plain `docker compose up -d --build` still works without make — Vite then
# falls back to a build timestamp (unique per build, so a deploy is still
# detected, just without the readable commit in the id).

export IRIS_WEB_BUILD_ID := $(shell git rev-parse --short HEAD 2>/dev/null)

.PHONY: deploy
deploy:
	docker compose $(ARGS) --profile cloudflared up -d --build
