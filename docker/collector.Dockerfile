# Placeholder image definition. Implementation language is intentionally undecided.
FROM alpine:3.20

RUN adduser -D -h /app app
WORKDIR /app

EXPOSE 8787
USER app

CMD ["sh", "-c", "echo 'collector implementation pending'; sleep infinity"]

