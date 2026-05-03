FROM python:3.12-alpine

WORKDIR /app

COPY target/debug/mise /usr/local/bin/mise
COPY oats/otel-basic/fixtures/mise.toml /app/mise.toml
COPY oats/otel-basic/fixtures/run-task.sh /app/run-task.sh
COPY oats/otel-basic/fixtures/server.py /app/server.py

RUN chmod +x /usr/local/bin/mise /app/run-task.sh

ENV MISE_TRUSTED_CONFIG_PATHS=/app

CMD ["python3", "/app/server.py"]
