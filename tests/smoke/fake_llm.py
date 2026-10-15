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
            if not prompt.startswith("Stage: fact composition"):
                raise ValueError("Unknown generation stage")
            groups = json.loads(
                prompt.split("\n\nExtracted evidence groups:\n", 1)[1].split(
                    "\n\nReturn JSON", 1
                )[0]
            )
            if len(groups) != 1:
                raise ValueError("Expected one evidence group")
            properties = request["response_format"]["json_schema"]["schema"]["properties"]
            result = {}
            for field in fields:
                slots = {}
                for source in groups[0]["sources"]:
                    key = source["source_key"]
                    schema = properties[field]["properties"].get(key)
                    if schema is None:
                        continue
                    if None in schema["enum"]:
                        slots[key] = None
                    else:
                        candidates = [fact for fact in source["facts"] if fact["field"] == field]
                        selected = min(
                            candidates,
                            key=lambda fact: (
                                not fact["reported"].startswith("tests.0: "),
                                fact["rendered_characters"],
                            ),
                        )
                        slots[key] = selected["id"]
                result[field] = slots
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
