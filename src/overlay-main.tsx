import React from "react";
import ReactDOM from "react-dom/client";
import "./pages/Overlay.css";
import OverlayPage from "./pages/Overlay";
import { IconSprite } from "./components/IconSprite";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <IconSprite />
    <OverlayPage />
  </React.StrictMode>,
);
