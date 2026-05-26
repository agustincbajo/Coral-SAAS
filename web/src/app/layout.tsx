import type { Metadata } from "next";
import "./globals.css";
import "@playsistemico/modo-bo-ui-lib/styles";
import { Providers } from "./providers";

export const metadata: Metadata = {
  title: "Coral",
  description: "AI-readable wiki for your codebase.",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
