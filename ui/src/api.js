import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, emit } from "@tauri-apps/api/event";

export { listen, emit, convertFileSrc };

export const api = {
  boot: () => invoke("boot"),
  list: () => invoke("vault_list"),
  chooseVault: () => invoke("vault_choose"),
  switchVault: (path) => invoke("vault_switch", { path }),
  forgetVault: (path) => invoke("vault_forget", { path }),
  read: (path) => invoke("note_read", { path }),
  write: (path, text) => invoke("note_write", { path, text }),
  stat: (path) => invoke("note_stat", { path }),
  create: (dir, name, text = "") => invoke("note_create", { dir, name, text }),
  createFolder: (dir, name) => invoke("folder_create", { dir, name }),
  rename: (from, to) => invoke("entry_rename", { from, to }),
  trash: (path) => invoke("entry_trash", { path }),
  search: (query) => invoke("vault_search", { query }),
  backlinks: (path) => invoke("note_backlinks", { path }),
  saveAttachment: (name, bytes) =>
    invoke("attachment_save", bytes, { headers: { "x-name": encodeURIComponent(name) } }),
  reveal: (path) => invoke("reveal", { path }),
  openExternal: (url) => invoke("open_external", { url }),
  saveUi: (patch) => invoke("settings_ui", { patch }),
  appearance: (background, dark) => invoke("set_appearance", { background, dark }),
  ready: () => invoke("ui_ready"),
  windowAction: (action) => invoke("window_action", { action }),
  // Voice builds only.
  voice: {
    state: () => invoke("voice_state"),
    settings: (patch) => invoke("voice_settings", { patch }),
    history: () => invoke("voice_history"),
    deleteHistory: (id) => invoke("voice_history_delete", { id }),
    clearHistory: () => invoke("voice_history_clear"),
    download: () => invoke("voice_model_download"),
    cancelDownload: () => invoke("voice_model_cancel"),
    deleteModel: () => invoke("voice_model_delete"),
    devices: () => invoke("voice_devices"),
    toggle: () => invoke("voice_toggle"),
    cancel: () => invoke("voice_cancel"),
    meeting: () => invoke("meeting_toggle"),
    hover: (width, height) => invoke("pill_area", { width, height }),
    copy: (text) => invoke("voice_copy", { text }),
    permissions: (request) => invoke("voice_permissions", { request }),
  },
};
