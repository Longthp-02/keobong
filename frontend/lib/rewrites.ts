/**
 * Proxies `/api/*` on the web app to the backend. Sign-in redirects and the
 * session cookie then live on the site's own domain (daghep.vn), so the cookie
 * is first-party and the Google redirect URI can be https://daghep.vn/api/....
 */
export function apiRewrites(apiBaseUrl: string | undefined) {
  if (!apiBaseUrl) {
    return [];
  }
  const base = apiBaseUrl.replace(/\/+$/, "");
  return [{ source: "/api/:path*", destination: `${base}/api/:path*` }];
}
