import React from "react";
import ReactDOM from "react-dom/client";
import "./pages/Overlay.css";
import OverlayPage from "./pages/Overlay";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <OverlayPage />
  </React.StrictMode>,
);
