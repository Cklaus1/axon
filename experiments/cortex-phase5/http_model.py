"""OpenAI-compatible HTTP model with the same interface as the Phase-1 local
Model, so the runners are unchanged.

Phases 1-4 loaded Qwen2.5-Coder-7B into this process. That stopped being
possible: two inference servers (vLLM, SGLang) already hold ~97/98 GB and
~92/98 GB of the two GPUs, so a third process blocks forever waiting for VRAM —
which is exactly how the first Phase-5 calibration hung at 0% CPU with no
output. Using the already-served model removes the contention AND changes the
model, which is a deliberate second generalization axis: every earlier result is
one 7B model.
"""
import json, time, urllib.request


class HttpModel:
    def __init__(self, base="http://127.0.0.1:18307/v1", name="Qwen3.8-Flash-Next"):
        self.base, self.name = base, name
        self.dtype = "server-managed"
        self.load_s = 0.0

    def generate_sampled(self, prompt, max_new_tokens, seed,
                         temperature=0.7, top_p=0.95):
        body = {"model": self.name,
                "messages": [{"role": "user", "content": prompt}],
                "max_tokens": max_new_tokens, "temperature": temperature,
                "top_p": top_p, "seed": seed}
        req = urllib.request.Request(
            f"{self.base}/chat/completions", data=json.dumps(body).encode(),
            headers={"Content-Type": "application/json"})
        t0 = time.time()
        with urllib.request.urlopen(req, timeout=300) as r:
            d = json.load(r)
        wall = time.time() - t0
        u = d.get("usage") or {}
        return d["choices"][0]["message"]["content"], {
            "prompt_tokens": u.get("prompt_tokens", 0),
            "completion_tokens": u.get("completion_tokens", 0),
            "gen_wall_s": round(wall, 3),
            "decoding": "sampled", "temperature": temperature, "top_p": top_p,
        }
