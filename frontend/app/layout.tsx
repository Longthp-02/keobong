import type { Metadata, Viewport } from "next";
import type { ReactNode } from "react";
import messages from "../messages/vi.json";
import "./globals.css";

const LINKEDIN_URL = "https://www.linkedin.com/in/long-pham-55466920b/";

export const metadata: Metadata = {
  title: messages.app.name,
  description: messages.app.tagline,
};

export const viewport: Viewport = {
  themeColor: "#1F7A3D",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="vi">
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="" />
        <link
          rel="stylesheet"
          href="https://fonts.googleapis.com/css2?family=Barlow+Condensed:wght@600;700&family=Be+Vietnam+Pro:wght@400;500;600;700&display=swap"
        />
      </head>
      <body>
        <div className="shell">
          <header className="topbar">
            <a href="/" className="logo">
              {messages.app.name.toUpperCase()}
              <span className="logo__dot">.</span>
            </a>
          </header>
          <main>{children}</main>
          <footer className="footer">
            <a href={LINKEDIN_URL} target="_blank" rel="noopener noreferrer">
              {messages.app.footer}
            </a>
          </footer>
        </div>
      </body>
    </html>
  );
}
