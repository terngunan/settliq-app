/**
 * Cloudflare Worker — status.settliq.xyz
 *
 * Routes:
 *   GET /          → serves the status page HTML (with API_BASE injected)
 *   GET /api/status → proxies GET https://api.settliq.xyz/health/admin
 *                     (avoids CORS issues; adds cache-control)
 *
 * Deploy:
 *   wrangler deploy
 *
 * Environment variables (set in wrangler.toml or Cloudflare dashboard):
 *   API_ORIGIN  = https://api.settliq.xyz   (no trailing slash)
 */

// ─── HTML is inlined at build time via wrangler's text_blobs or fetched ───────
// For simplicity we import it as a string. In wrangler.toml add:
//   [text_blobs]
//   STATUS_HTML = "index.html"
// Then reference it as STATUS_HTML below.
// If you prefer, replace with a fetch to your CDN.

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    // ── Proxy: /api/status → upstream /health/admin ──────────────────────────
    if (url.pathname === '/api/status') {
      return proxyHealthAdmin(env);
    }

    // ── Status page HTML ──────────────────────────────────────────────────────
    if (url.pathname === '/' || url.pathname === '/index.html') {
      return serveStatusPage(env);
    }

    // ── Favicon (prevent 404 noise) ───────────────────────────────────────────
    if (url.pathname === '/favicon.ico') {
      return new Response(null, { status: 204 });
    }

    return new Response('Not found', { status: 404 });
  },
};

// ─── Proxy /health/admin ──────────────────────────────────────────────────────
async function proxyHealthAdmin(env) {
  const apiOrigin = env.API_ORIGIN ?? 'https://api.settliq.xyz';

  let upstreamRes;
  try {
    upstreamRes = await fetch(`${apiOrigin}/health/admin`, {
      headers: { Accept: 'application/json' },
      // Fail fast (5s) instead of the 30s default subrequest limit so a stalled
      // upstream doesn't hang the worker and exhaust concurrent execution.
      signal: AbortSignal.timeout(5000),
    });
  } catch (err) {
    // Network-level failure or timeout — API is unreachable
    return jsonResponse(
      {
        status: 'down',
        timestamp: new Date().toISOString(),
        components: {
          database:   { status: 'down', latency: 0 },
          stellar:    { status: 'down', latency: 0 },
          partnerApi: { status: 'down', latency: 0 },
          redis:      { status: 'down', latency: 0 },
          queue:      { status: 'down', latency: 0 },
        },
        error: 'API unreachable',
      },
      503,
    );
  }

  // Parse body regardless of status code (/health/admin returns 503 with body
  // when critical components are down)
  let body;
  try {
    body = await upstreamRes.json();
  } catch {
    body = { status: 'down', timestamp: new Date().toISOString(), error: 'Invalid response' };
  }

  return jsonResponse(body, upstreamRes.status, {
    // Cache for 30 seconds at the edge — status page polls every 60s so this
    // halves upstream load while keeping data fresh enough.
    'Cache-Control': 'public, max-age=30, s-maxage=30',
  });
}

// ─── Serve status page HTML ───────────────────────────────────────────────────
async function serveStatusPage(env) {
  // STATUS_HTML is bound as a text blob in wrangler.toml and is available on
  // the env object in ES module workers (not on the global scope).
  // Fallback: serve a minimal stub when the binding is absent (local dev).
  const html = env.STATUS_HTML ?? (await fetchFallbackHtml());

  // Inject the API base so the page calls /api/status (this worker's proxy)
  // instead of the real API directly — avoids CORS entirely.
  const injected = html.replace(
    "window.__API_BASE__ || 'https://api.settliq.xyz'",
    "window.__API_BASE__ || ''",  // empty string → relative URL /api/status
  ).replace(
    "`${API_BASE}/health/admin`",
    "`${API_BASE}/api/status`",
  );

  return new Response(injected, {
    headers: {
      'Content-Type': 'text/html; charset=utf-8',
      'Cache-Control': 'public, max-age=60',
      'X-Content-Type-Options': 'nosniff',
      'X-Frame-Options': 'DENY',
      'Referrer-Policy': 'strict-origin-when-cross-origin',
    },
  });
}

// ─── Helpers ──────────────────────────────────────────────────────────────────
function jsonResponse(body, status = 200, extraHeaders = {}) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      'Content-Type': 'application/json',
      'Access-Control-Allow-Origin': '*',
      ...extraHeaders,
    },
  });
}

async function fetchFallbackHtml() {
  // Only used in local wrangler dev when text_blobs aren't available
  return '<html><body>Status page loading...</body></html>';
}
