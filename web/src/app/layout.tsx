import type { Metadata } from "next";
import "./globals.css";
import "@playsistemico/modo-bo-ui-lib/styles";

export const metadata: Metadata = {
  title: "Coral",
  description: "AI-readable wiki for your codebase.",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
