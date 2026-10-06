import type { NextConfig } from "next";
const apiOrigin = process.env.API_ORIGIN ?? "http://127.0.0.1:8080";
const config: NextConfig = {
  async rewrites() {
    return [
      { source: "/health/:path*", destination: `${apiOrigin}/health/:path*` },
      { source: "/api/v1/:path*", destination: `${apiOrigin}/api/v1/:path*` },
    ];
  },
};
export default config;
