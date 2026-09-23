import json
import re
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
        evidence = prompt.partition("Evidence groups:\n")[2].partition(
            "\n\nReturn JSON"
        )[0]
        group = re.search(r'"group_id"\s*:\s*"([^"]+)"', evidence)
        if not group:
            self.send_error(422)
            return
        content = json.dumps(
            {
                "title": "Compose smoke workflow",
                "outcome": ["Validated the live Daily workflow."],
                "decision": [],
                "trade_off": [],
                "validation": ["The live Compose smoke test passed."],
                "blocker": [],
                "follow_up": [],
                "open_questions": [],
            }
        )
        body = json.dumps({"choices": [{"message": {"content": content}}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format, *_args):
        return


ThreadingHTTPServer(("0.0.0.0", 8000), Handler).serve_forever()
