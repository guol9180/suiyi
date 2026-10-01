import React from "react";
import ReactDOM from "react-dom/client";
// 覆盖层是独立窗口，tokens 不会跟着主窗口过来，必须自己引一次
import "./styles/tokens.css";
import "./pages/Overlay.css";
import OverlayPage from "./pages/Overlay";
import { IconSprite } from "./components/IconSprite";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <IconSprite />
    <OverlayPage />
  </React.StrictMode>,
);
