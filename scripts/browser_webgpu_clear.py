#!/usr/bin/env python3
# R7c Slice 3 — drive headless Chrome via the raw chromedriver wire protocol
# (no selenium), navigate to the WebGPU clear fixture, poll window.__done, print
# the result. Exits 0 and prints "CLEAR_RGBA=..." on success.
import json, os, subprocess, sys, time, urllib.request

CHROMEDRIVER = os.environ.get("CHROMEDRIVER", "chromedriver")
PORT = int(os.environ.get("WEBGPU_DRIVER_PORT", "39517"))
URL = "file://" + os.path.abspath(sys.argv[1])

drv = subprocess.Popen([CHROMEDRIVER, f"--port={PORT}"],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
time.sleep(2)
base = f"http://127.0.0.1:{PORT}"


def post(path, body):
    req = urllib.request.Request(base + path, data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(req, timeout=30))


try:
    # SwiftShader-Vulkan WebGPU under headless: the documented software path.
    caps = {"capabilities": {"alwaysMatch": {"goog:chromeOptions": {"args": [
        "--headless=new", "--no-sandbox", "--disable-dev-shm-usage",
        "--enable-unsafe-webgpu", "--enable-features=Vulkan",
        "--use-angle=vulkan", "--use-vulkan=swiftshader",
    ]}}}}
    sess = post("/session", caps)
    sid = sess["value"]["sessionId"]
    post(f"/session/{sid}/url", {"url": URL})
    result = "TIMEOUT"
    for _ in range(40):  # up to ~20s
        r = post(f"/session/{sid}/execute/sync",
                 {"script": "return window.__done;", "args": []})
        v = r.get("value")
        if v and v != "PENDING":
            result = v
            break
        time.sleep(0.5)
    print(result)
    try:
        urllib.request.urlopen(urllib.request.Request(
            base + f"/session/{sid}", method="DELETE"), timeout=10)
    except Exception:
        pass
    sys.exit(0 if result.startswith("CLEAR_RGBA=") else 1)
finally:
    drv.terminate()
