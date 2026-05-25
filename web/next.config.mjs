/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // Standalone output makes the Docker image ~10x smaller.
  output: "standalone",
  // The modo-bo-ui-lib ships ESM + UMD; transpile it through Next's compiler
  // so SSR doesn't choke on raw ESM exports.
  transpilePackages: ["@playsistemico/modo-bo-ui-lib"],
  experimental: {
    // Reserved for future tweaks (PPR, etc.). Empty for now.
  },
};

export default nextConfig;
