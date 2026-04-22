#!/usr/bin/env python3
"""porty — a fast concurrent TCP port scanner."""

import argparse
import socket
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed

COMMON_PORTS = [
    21, 22, 23, 25, 53, 80, 110, 111, 135, 139, 143, 443, 445,
    465, 587, 631, 993, 995, 1080, 1433, 1521, 2049, 2181, 3000,
    3306, 3389, 4369, 5432, 5672, 5900, 6379, 6443, 8080, 8443,
    8888, 9000, 9092, 9200, 11211, 27017, 50070,
]

SERVICES = {
    21: "ftp", 22: "ssh", 23: "telnet", 25: "smtp", 53: "dns",
    80: "http", 110: "pop3", 111: "rpcbind", 135: "msrpc",
    139: "netbios", 143: "imap", 443: "https", 445: "smb",
    465: "smtps", 587: "submission", 631: "ipp", 993: "imaps",
    995: "pop3s", 1080: "socks", 1433: "mssql", 1521: "oracle",
    2049: "nfs", 2181: "zookeeper", 3000: "dev-server",
    3306: "mysql", 3389: "rdp", 4369: "epmd", 5432: "postgres",
    5672: "amqp", 5900: "vnc", 6379: "redis", 6443: "k8s-api",
    8080: "http-alt", 8443: "https-alt", 8888: "jupyter",
    9000: "sonarqube", 9092: "kafka", 9200: "elasticsearch",
    11211: "memcached", 27017: "mongodb", 50070: "hdfs",
}


def probe(host: str, port: int, timeout: float) -> tuple[int, bool]:
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return port, True
    except (ConnectionRefusedError, socket.timeout, OSError):
        return port, False


def resolve(host: str) -> str:
    try:
        return socket.gethostbyname(host)
    except socket.gaierror as e:
        sys.exit(f"Cannot resolve {host!r}: {e}")


def parse_ports(spec: str) -> list[int]:
    ports = set()
    for part in spec.split(","):
        part = part.strip()
        if "-" in part:
            lo, hi = part.split("-", 1)
            ports.update(range(int(lo), int(hi) + 1))
        else:
            ports.add(int(part))
    return sorted(ports)


def scan(host: str, ports: list[int], workers: int, timeout: float) -> list[int]:
    open_ports = []
    with ThreadPoolExecutor(max_workers=workers) as pool:
        futures = {pool.submit(probe, host, p, timeout): p for p in ports}
        for future in as_completed(futures):
            port, is_open = future.result()
            if is_open:
                open_ports.append(port)
    return sorted(open_ports)


def main():
    parser = argparse.ArgumentParser(
        prog="porty",
        description="Fast concurrent TCP port scanner",
    )
    parser.add_argument("host", help="Target hostname or IP address")
    parser.add_argument(
        "-p", "--ports",
        default="common",
        help='Ports to scan: "common" (default), "all", or a list like "22,80,443,8000-9000"',
    )
    parser.add_argument("-w", "--workers", type=int, default=256, help="Concurrent threads (default: 256)")
    parser.add_argument("-t", "--timeout", type=float, default=1.0, help="Connect timeout in seconds (default: 1.0)")
    args = parser.parse_args()

    if args.ports == "common":
        ports = COMMON_PORTS
    elif args.ports == "all":
        ports = list(range(1, 65536))
    else:
        try:
            ports = parse_ports(args.ports)
        except ValueError:
            sys.exit(f"Invalid port spec: {args.ports!r}")

    ip = resolve(args.host)
    label = f"{args.host} ({ip})" if args.host != ip else ip
    print(f"Scanning {label}  [{len(ports)} port(s), {args.workers} threads, {args.timeout}s timeout]")

    open_ports = scan(ip, ports, args.workers, args.timeout)

    if not open_ports:
        print("No open ports found.")
        return

    width = max(len(str(p)) for p in open_ports)
    print(f"\n{'PORT':<{width + 2}}  SERVICE")
    print("-" * 30)
    for port in open_ports:
        service = SERVICES.get(port, "unknown")
        print(f"{port:<{width + 2}}  {service}")
    print(f"\n{len(open_ports)} open port(s) found.")


if __name__ == "__main__":
    main()
