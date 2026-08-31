# Placeholder image definition. Implementation language is intentionally undecided.
FROM alpine:3.20

RUN adduser -D -h /app app
WORKDIR /app

EXPOSE 8788
USER app

CMD ["sh", "-c", "echo 'mcp implementation pending'; sleep infinity"]

