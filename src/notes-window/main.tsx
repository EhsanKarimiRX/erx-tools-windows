import React from "react";
import ReactDOM from "react-dom/client";
import NotesWindowApp from "./NotesWindowApp";
import "../styles/global.css";
import "./notes-window.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <NotesWindowApp />
  </React.StrictMode>,
);
