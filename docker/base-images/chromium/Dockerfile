ARG BASE_TAG=latest
ARG REGISTRY=browseruse
FROM ${REGISTRY}/base-system:${BASE_TAG}

ENV PLAYWRIGHT_BROWSERS_PATH=/opt/playwright

# Pin the installer and ask it for the executable path: Chromium distributions
# have used both chrome-linux and chrome-linux64 directories.
RUN --mount=type=cache,target=/root/.cache,sharing=locked \
    uvx --from playwright==1.63.0 playwright install chromium --with-deps --no-shell && \
    uvx --from playwright==1.63.0 python -c 'import os; from playwright.sync_api import sync_playwright; p = sync_playwright().start(); os.symlink(p.chromium.executable_path, "/usr/bin/chromium-browser"); p.stop()' && \
    test -x /usr/bin/chromium-browser && \
    chmod -R 755 /opt/playwright
