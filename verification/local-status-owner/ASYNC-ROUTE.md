# Asynchronous local owner route

The Rust overlay can set `async_route_deadline_ms` when `routed_requests` is enabled. The generated server retains the synchronous methods and adds `serve_async_routed_until` for a bounded, awaited reply. Status remains independent of a pending route. The response length and peer checks still apply.

The river gauge fixture exercises an awaited reading, a timed-out route alongside a valid status request, and socket cleanup. The existing status and synchronous routing checks also passed on macOS ARM64.

```sh
python3 -B verification/local-status-owner/check.py /path/to/oddlang
```
