(() => {
  "use strict";

  const body = document.body;
  const toc = document.querySelector("#napkin-toc");
  const menuButton = document.querySelector("[data-napkin-menu-toggle]");
  const closeButton = document.querySelector("[data-napkin-menu-close]");
  const backdrop = document.querySelector("[data-napkin-menu-backdrop]");
  const sitebar = document.querySelector(".napkin-sitebar");
  const page = document.querySelector(".ltx_page_main");
  const drawerQuery = window.matchMedia("(max-width: 75rem)");

  if (!toc || !menuButton || !closeButton || !backdrop) {
    return;
  }

  let restoreFocus = null;

  const focusableElements = () =>
    Array.from(
      toc.querySelectorAll(
        'a[href], button:not([disabled]), [tabindex]:not([tabindex="-1"])'
      )
    ).filter((element) => !element.hidden);

  const setBackgroundInert = (inert) => {
    if (sitebar) {
      sitebar.inert = inert;
    }
    if (page) {
      page.inert = inert;
    }
  };

  const setMenuOpen = (open, returnFocus = true) => {
    if (!drawerQuery.matches) {
      body.classList.remove("napkin-menu-open");
      menuButton.setAttribute("aria-expanded", "false");
      toc.removeAttribute("aria-hidden");
      backdrop.hidden = true;
      setBackgroundInert(false);
      return;
    }

    body.classList.toggle("napkin-menu-open", open);
    menuButton.setAttribute("aria-expanded", String(open));
    toc.setAttribute("aria-hidden", String(!open));
    backdrop.hidden = !open;
    setBackgroundInert(open);

    if (open) {
      restoreFocus = document.activeElement;
      closeButton.focus({ preventScroll: true });
    } else if (returnFocus && restoreFocus instanceof HTMLElement) {
      restoreFocus.focus({ preventScroll: true });
      restoreFocus = null;
    }
  };

  menuButton.addEventListener("click", () => setMenuOpen(true));
  closeButton.addEventListener("click", () => setMenuOpen(false));
  backdrop.addEventListener("click", () => setMenuOpen(false));

  toc.addEventListener("click", (event) => {
    if (drawerQuery.matches && event.target.closest("a")) {
      setMenuOpen(false);
    }
  });

  document.addEventListener("keydown", (event) => {
    if (!body.classList.contains("napkin-menu-open")) {
      return;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      setMenuOpen(false);
      return;
    }

    if (event.key !== "Tab") {
      return;
    }

    const focusable = focusableElements();
    if (focusable.length === 0) {
      event.preventDefault();
      toc.focus();
      return;
    }

    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  });

  const syncMenuMode = () => setMenuOpen(false, false);
  drawerQuery.addEventListener("change", syncMenuMode);
  syncMenuMode();

  const currentEntry = toc.querySelector(".ltx_tocentry.ltx_ref_self");
  if (currentEntry) {
    currentEntry.classList.add("napkin-current-entry");
    currentEntry.querySelector(":scope > .ltx_ref_self")?.setAttribute(
      "aria-current",
      "page"
    );

    requestAnimationFrame(() => {
      const desiredTop = currentEntry.offsetTop - toc.clientHeight * 0.35;
      toc.scrollTop = Math.max(0, desiredTop);
    });
  }
})();
