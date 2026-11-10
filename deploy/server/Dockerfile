FROM python:3.11-slim

WORKDIR /app
COPY server/pyproject.toml server/README.md ./
COPY server/app ./app

RUN pip install --no-cache-dir .

ENV MI_SERVER_DATA_DIR=/var/lib/multinteractor \
    MI_SERVER_HOST=0.0.0.0 \
    MI_SERVER_PORT=8000 \
    MI_CACHE_TTL_SECONDS=86400 \
    MI_CACHE_CLEANUP_INTERVAL_SECONDS=60 \
    MI_UI_ENABLED=true

VOLUME ["/var/lib/multinteractor"]
EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD python3 -c "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8000/healthz', timeout=3)"

CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
