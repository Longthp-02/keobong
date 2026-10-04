import type { NextConfig } from "next";
import { apiRewrites } from "./lib/rewrites";

const nextConfig: NextConfig = {
  poweredByHeader: false,
  async rewrites() {
    return apiRewrites(process.env.API_BASE_URL);
  },
};

export default nextConfig;
