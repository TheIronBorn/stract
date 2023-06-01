#!.venv/bin/python3
import argparse
import subprocess

parser = argparse.ArgumentParser()

parser.add_argument("--release", action="store_true")
parser.add_argument("--alice", action="store_true")

args = parser.parse_args()

release = ["--release"] if args.release else []

processes = []

processes.append(
    subprocess.Popen(
        ["cargo", "run"] + release + ["--", "frontend", "configs/frontend.toml"]
    )
)
processes.append(
    subprocess.Popen(
        ["cargo", "run"]
        + release
        + ["--", "search-server", "configs/search_server.toml"]
    )
)
processes.append(
    subprocess.Popen(
        ["cargo", "run"]
        + release
        + [
            "--",
            "webgraph",
            "server",
            "configs/webgraph/server.toml",
        ]
    )
)

if args.alice:
    processes.append(
        subprocess.Popen(
            ["cargo", "run"] + release + ["--", "alice", "serve", "configs/alice.toml"]
        )
    )

# kill processes on ctrl-c
import time

while True:
    try:
        time.sleep(1)
    except KeyboardInterrupt:
        for p in processes:
            p.kill()
        break
