import type { MetadataRoute } from "next";
import messages from "../messages/vi.json";

// Makes the site installable ("Add to Home Screen"). Icons: TODO: verify once a logo exists.
export default function manifest(): MetadataRoute.Manifest {
  return {
    name: messages.app.name,
    short_name: messages.app.name,
    description: messages.app.tagline,
    start_url: "/",
    display: "standalone",
    background_color: "#F4F2EC",
    theme_color: "#1F7A3D",
    lang: "vi",
  };
}
