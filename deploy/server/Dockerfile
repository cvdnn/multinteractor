FROM python:3.11-slim

WORKDIR /app
COPY server/pyproject.toml server/README.md ./
COPY server/app ./app

RUN pip install --no-cache-dir .

ENV MI_SERVER_DATA_DIR=/var/lib/multinteractor
VOLUME ["/var/lib/multinteractor"]
EXPOSE 8000

CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
