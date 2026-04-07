import argparse
import asyncio
import json
import os
import re
import sys
from urllib.parse import quote


def _normalize_code(code: str) -> str:
    c = code.strip()
    if not c:
        return ""
    if "." in c:
        left, right = c.split(".", 1)
        if left and right:
            return f"{right}{left}".lower()
    return c


def build_sina_ws_url(codes: list[str]) -> str:
    normalized = [_normalize_code(c) for c in codes]
    normalized = [c for c in normalized if c]
    if not normalized:
        raise ValueError("codes is empty")
    joined = ",".join(normalized)
    return f"ws://w.sinajs.cn/wskt?list={quote(joined, safe=',')}"


def _disable_proxy_env() -> None:
    for k in (
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "WS_PROXY",
        "ws_proxy",
        "WSS_PROXY",
        "wss_proxy",
        "NO_PROXY",
        "no_proxy",
    ):
        os.environ.pop(k, None)


async def fetch_history(
    symbol: str,
    period: int,
    max_attempts: int = 3,
    timeout: float = 10.0,
) -> list[dict] | None:
    try:
        import aiohttp
    except Exception as e:
        sys.stderr.write(f"missing aiohttp: {e}\n")
        return None

    var_name = f"_{symbol}_{period}_{id(symbol)}"
    url = "https://gu.sina.cn/ft/api/jsonp.php/var%20" + quote(var_name) + "=/GlobalService.getMink"

    params = {"symbol": symbol, "type": str(period)}
    headers = {
        "Accept": "*/*",
        "Accept-Language": "en,zh-CN;q=0.9,zh;q=0.8,ja;q=0.7,zh-TW;q=0.6",
        "Referer": f"https://finance.sina.com.cn/futures/quotes/{symbol}.shtml",
        "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36",
    }

    _disable_proxy_env()

    for attempt in range(max_attempts):
        try:
            async with aiohttp.ClientSession() as session:
                async with session.get(
                    url,
                    params=params,
                    headers=headers,
                    timeout=aiohttp.ClientTimeout(total=timeout),
                ) as resp:
                    text = await resp.text()
        except Exception as e:
            sys.stderr.write(f"[attempt {attempt + 1}] {type(e).__name__}: {e}\n")
            continue

        sys.stderr.write(f"[attempt {attempt + 1}] raw length: {len(text)}\n")

        m = re.search(r"=\(\[", text)
        if not m:
            sys.stderr.write(f"[attempt {attempt + 1}] no =([ pattern, text: {text[:100]}\n")
            continue

        start = m.end() - 1
        depth = 1
        raw = None
        for i in range(start + 1, len(text)):
            ch = text[i]
            if ch == "(":
                depth += 1
            elif ch == ")":
                depth -= 1
                if depth == 0:
                    raw = text[start + 1:i]
                    break

        if not raw:
            sys.stderr.write(f"[attempt {attempt + 1}] failed to find matching parens\n")
            continue

        try:
            data = json.loads("[" + raw)
            if data and isinstance(data, list):
                result = []
                for item in data:
                    if len(item) >= 6:
                        result.append({
                            "time": item["d"],
                            "open": item["o"],
                            "close": item["c"],
                            "high": item["h"],
                            "low": item["l"],
                            "volume": item["v"],
                        })
                return result
        except json.JSONDecodeError as e:
            sys.stderr.write(f"[attempt {attempt + 1}] JSON error: {e}\n")
            continue

        sys.stderr.write(f"[attempt {attempt + 1}] empty data\n")
        continue

    sys.stderr.write(f"failed after {max_attempts} attempts\n")
    return None


async def run(url: str, max_messages: int, seconds: float, use_proxy: bool) -> None:
    try:
        import websockets  # type: ignore
    except Exception:
        sys.stderr.write("缺少依赖: websockets\n")
        sys.stderr.write("安装: python3 -m pip install websockets\n")
        raise

    if not use_proxy:
        _disable_proxy_env()

    async def _recv_loop(ws) -> None:
        count = 0
        while True:
            msg = await ws.recv()
            if isinstance(msg, (bytes, bytearray)):
                sys.stdout.write(f"[binary] {len(msg)} bytes\n")
                sys.stdout.flush()
                continue
            text = str(msg)
            for line in text.splitlines():
                if line.strip():
                    sys.stdout.write(line + "\n")
            sys.stdout.flush()
            count += 1
            if max_messages > 0 and count >= max_messages:
                return

    async with websockets.connect(
        url,
        origin="https://gu.sina.cn",
        extra_headers={
            "Host": "w.sinajs.cn",
        },
        user_agent_header="Mozilla/5.0",
        ping_interval=60,
        ping_timeout=3,
        close_timeout=3,
    ) as ws:
        if seconds > 0:
            await asyncio.wait_for(_recv_loop(ws), timeout=seconds)
        else:
            await _recv_loop(ws)


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument(
        "--codes",
        default="sz000001",
        help="逗号分隔。支持: 000001.SZ / 600000.SH / sz000001 / hf_OIL 这种格式",
    )
    p.add_argument("--max-messages", type=int, default=0, help="收到多少条WebSocket消息后退出，0表示不限制")
    p.add_argument("--seconds", type=float, default=20.0, help="运行多久后退出，<=0表示不限制")
    p.add_argument("--use-proxy", action="store_true", help="是否使用环境变量里的代理(默认不使用)")
    p.add_argument(
        "--history",
        metavar="SYMBOL",
        help="获取历史行情，如: --history OIL (获取国际原油5分钟K线)",
    )
    p.add_argument(
        "--period",
        type=int,
        default=5,
        help="历史行情周期(分钟)，默认5分钟",
    )
    args = p.parse_args()

    if args.history:
        return _run_history(args.history, args.period)

    codes = [c.strip() for c in args.codes.split(",") if c.strip()]
    url = build_sina_ws_url(codes)
    sys.stdout.write(f"url={url}\n")
    sys.stdout.flush()

    try:
        asyncio.run(run(url, args.max_messages, args.seconds, args.use_proxy))
        return 0
    except asyncio.TimeoutError:
        return 0
    except KeyboardInterrupt:
        return 0
    except Exception as e:
        sys.stderr.write(f"error: {e}\n")
        return 1


def _run_history(symbol: str, period: int) -> int:
    sys.stdout.write(f"fetching {symbol} {period}min history...\n")
    sys.stdout.flush()

    async def _main() -> int:
        data = await fetch_history(symbol, period)
        if data is None:
            return 1
        sys.stdout.write(f"got {len(data)} records\n")
        for row in data[:5]:
            sys.stdout.write(f"  {row}\n")
        if len(data) > 5:
            sys.stdout.write(f"  ... ({len(data) - 5} more)\n")
        sys.stdout.flush()
        return 0

    try:
        return asyncio.run(_main())
    except KeyboardInterrupt:
        return 0
    except Exception as e:
        sys.stderr.write(f"error: {e}\n")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
