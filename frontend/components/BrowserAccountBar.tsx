"use client";

import { useEffect, useState } from "react";
import { type MeView, browserMe } from "../lib/api";
import { AccountBar } from "./AccountBar";

type Props = { loadMe?: () => Promise<MeView | null> };

/**
 * The account bar for pages served from the CDN cache: who is signed in is
 * asked from the browser after loading, so the cached HTML stays the same for everyone.
 */
export function BrowserAccountBar({ loadMe = browserMe }: Props) {
  const [me, setMe] = useState<MeView | null>(null);

  useEffect(() => {
    let active = true;
    loadMe().then(
      (user) => {
        if (active) {
          setMe(user);
        }
      },
      // Only a convenience on this page; joining still reports sign-in problems itself.
      (error: unknown) => console.error("could not load the signed-in user", error),
    );
    return () => {
      active = false;
    };
  }, [loadMe]);

  return me ? <AccountBar me={me} /> : null;
}
