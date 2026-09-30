import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/tokens.css";
import "./styles/base.css";
import "./pages/Popup.css";
import PopupPage from "./pages/Popup";
import { IconSprite } from "./components/IconSprite";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <IconSprite />
    <PopupPage />
  </React.StrictMode>,
);
