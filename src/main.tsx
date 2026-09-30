import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/tokens.css";
import "./styles/base.css";
import App from "./App";
import { IconSprite } from "./components/IconSprite";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <IconSprite />
    <App />
  </React.StrictMode>,
);
