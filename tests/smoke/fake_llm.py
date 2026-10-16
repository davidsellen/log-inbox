import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_POST(self):
        length = min(int(self.headers.get("Content-Length", "0")), 1024 * 1024)
        request = json.loads(self.rfile.read(length))
        messages = request.get("messages", [])
        prompt = next(
            (item.get("content", "") for item in messages if item.get("role") == "user"),
            "",
        )
        fields = ("outcome", "decision", "trade_off", "validation", "blocker", "follow_up")
        try:
            properties = request["response_format"]["json_schema"]["schema"]["properties"]
            if prompt.startswith("Stage: topic partition"):
                fact_ids = properties["topics"]["items"]["properties"]["fact_ids"]["items"]["enum"]
                result = {"topics": [{"subject": "Synthetic subject", "fact_ids": fact_ids}]}
            elif prompt.startswith("Stage: fact composition"):
                groups = json.loads(
                    prompt.split("\n\nExtracted evidence groups:\n", 1)[1].split(
                        "\n\nReturn JSON", 1
                    )[0]
                )
                if len(groups) != 1:
                    raise ValueError("Expected one evidence group")
                facts = {row[0]: {"topic": row[1], "human_display": row[4]} for row in groups[0]["facts"]}
                choices = [{"id": row[0], "field": fields[row[1]], "fact_ids": row[2],
                            "topic_ids": {facts[fact]["topic"] for fact in row[2]}}
                           for row in groups[0]["bundle_choices"]]
                result = {}
                for field in fields:
                    if properties[field]["maxItems"] == 0:
                        result[field] = []
                        continue
                    authorized = properties[field]["items"]["enum"]
                    candidates = [choice for choice in choices if choice["field"] == field and choice["id"] in authorized]
                    needed = {topic for row in groups[0]["mandatory_topic_fields"] if fields[row[0]] == field for topic in row[1]}
                    if not needed:
                        needed = {topic for choice in candidates for topic in choice["topic_ids"]}
                    used = set()
                    selected = []
                    while needed and len(selected) < 4:
                        choice = max(
                            (choice for choice in candidates if not used.intersection(choice["fact_ids"])),
                            key=lambda choice: (
                                len(needed.intersection(choice["topic_ids"])),
                                any("0: live Compose smoke" in facts[fact_id]["human_display"] for fact_id in choice["fact_ids"]),
                                len(choice["fact_ids"]),
                            ),
                        )
                        needed.difference_update(choice["topic_ids"])
                        used.update(choice["fact_ids"])
                        selected.append(choice["id"])
                    if needed:
                        raise ValueError("Fixture exceeds four-fact topic coverage")
                    result[field] = selected
            else:
                raise ValueError("Unknown generation stage")
        except (ValueError, KeyError, IndexError, TypeError):
            self.send_error(422)
            return
        content = json.dumps(result)
        body = json.dumps({"choices": [{"message": {"content": content}}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format, *_args):
        return


ThreadingHTTPServer(("0.0.0.0", 8000), Handler).serve_forever()
