import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/tokens.css";
import "./styles/base.css";
import "./pages/OcrResult.css";
import OcrResultPage from "./pages/OcrResult";
import { IconSprite } from "./components/IconSprite";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <IconSprite />
    <OcrResultPage />
  </React.StrictMode>,
);
