FROM python:3.12-slim

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    COLLECTOR_HOST=0.0.0.0 \
    COLLECTOR_PORT=8080 \
    COLLECTOR_DB=/data/collector.sqlite3

WORKDIR /app

RUN useradd --create-home --shell /usr/sbin/nologin collector \
    && mkdir -p /data \
    && chown -R collector:collector /data /app

COPY collector_server ./collector_server

USER collector
EXPOSE 8080

CMD ["python", "-m", "collector_server"]
