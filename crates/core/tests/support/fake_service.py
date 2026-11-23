"""Stands in for mysqld, php-fpm, nginx and mailpit during tests.

Usage: fake_service.py <role> <the real program's arguments...>
It opens the port or socket the real program would, then waits for SIGTERM.
"""
import os, re, signal, socket, sys, time

role, args = sys.argv[1], sys.argv[2:]

def read(path):
    with open(path) as f:
        return f.read()

def tcp(port):
    s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", int(port))); s.listen(8); return s

if role == "mysqld":
    conf = [a for a in args if a.startswith("--defaults-file=")][0].split("=", 1)[1]
    listener = tcp(re.search(r"port\s*=\s*(\d+)", read(conf)).group(1))
elif role == "nginx":
    conf = args[args.index("-c") + 1]
    listener = tcp(re.search(r"port=(\d+)", read(conf)).group(1))
elif role == "php-fpm":
    conf = args[args.index("-y") + 1]
    path = re.search(r"listen=(.*)", read(conf)).group(1).strip()
    if os.path.exists(path): os.remove(path)
    listener = socket.socket(socket.AF_UNIX); listener.bind(path); listener.listen(8)
elif role == "mailpit":
    listener = tcp(args[args.index("--listen") + 1].split(":")[1])
elif role == "crash":
    print("fake failure: cannot open datadir", file=sys.stderr); sys.exit(1)
else:
    sys.exit("unknown role")

signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
listener.settimeout(0.2)
while True:
    try:
        c, _ = listener.accept(); c.close()
    except socket.timeout:
        pass
