import { mount } from "svelte";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.svelte";

mount(App, { target: document.getElementById("app")! });
requestAnimationFrame(() => requestAnimationFrame(() => invoke("app_ready")));
