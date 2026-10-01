import { useState } from "react";
import type { LibraryPageMode } from "../app-state";
import type { ServerWorkspaceSection, ViewKey } from "../types";

export function useDesktopUiState(options: {
  onSelectInstance: (instanceId: string) => void;
  onSelectModule: (moduleId: string) => void;
}) {
  const [activeView, setActiveView] = useState<ViewKey>("system");
  const [libraryPage, setLibraryPage] = useState<LibraryPageMode>("catalog");
  const [libraryCatalogFocusId, setLibraryCatalogFocusId] = useState<string | null>(null);
  const [libraryCatalogScrollLeft, setLibraryCatalogScrollLeft] = useState(0);
  const [librarySearch, setLibrarySearch] = useState("");
  const [serverWorkspaceSection, setServerWorkspaceSection] = useState<ServerWorkspaceSection>("overview");

  function openView(view: ViewKey) {
    setActiveView(view);
  }

  function openServerWorkspace(section: ServerWorkspaceSection = "overview") {
    setServerWorkspaceSection(section);
    openView("servers");
  }

  function openInstanceView(section: ServerWorkspaceSection, instanceId: string) {
    options.onSelectInstance(instanceId);
    openServerWorkspace(section);
  }

  function openLibraryCatalog() {
    setLibraryPage("catalog");
    openView("library");
  }

  function openLibraryDetail(moduleId: string) {
    options.onSelectModule(moduleId);
    setLibraryCatalogFocusId(moduleId);
    setLibraryPage("detail");
    openView("library");
  }

  function handleNavSelect(nextView: ViewKey) {
    if (nextView === "library") {
      setLibraryPage("catalog");
    }

    if (nextView === "servers") {
      setServerWorkspaceSection("overview");
    }

    openView(nextView);
  }

  function handleSearchChange(value: string) {
    if (activeView !== "library") {
      return;
    }

    setLibrarySearch(value);
    if (libraryPage === "detail") {
      setLibraryPage("catalog");
    }
  }

  return {
    activeView,
    libraryCatalogFocusId,
    libraryCatalogScrollLeft,
    libraryPage,
    librarySearch,
    openInstanceView,
    openLibraryCatalog,
    openLibraryDetail,
    openServerWorkspace,
    openView,
    serverWorkspaceSection,
    handleNavSelect,
    handleSearchChange,
    setLibraryCatalogFocusId,
    setLibraryCatalogScrollLeft
  };
}
