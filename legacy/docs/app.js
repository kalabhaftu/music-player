(function () {
  const root = document.documentElement;
  const searchInput = document.getElementById("doc-search");
  const themeBtn = document.getElementById("theme-toggle");
  const navLinks = Array.from(document.querySelectorAll(".nav-link"));
  const sections = Array.from(document.querySelectorAll(".doc-section"));

  const savedTheme = localStorage.getItem("mp-doc-theme");
  if (savedTheme === "light" || savedTheme === "dark") {
    root.setAttribute("data-theme", savedTheme);
  }

  themeBtn.addEventListener("click", () => {
    const next = root.getAttribute("data-theme") === "dark" ? "light" : "dark";
    root.setAttribute("data-theme", next);
    localStorage.setItem("mp-doc-theme", next);
  });

  function updateActiveNav() {
    let activeId = sections[0]?.id || "";
    const threshold = window.scrollY + 120;
    for (const section of sections) {
      if (!section.classList.contains("hidden") && section.offsetTop <= threshold) {
        activeId = section.id;
      }
    }
    for (const link of navLinks) {
      const target = link.getAttribute("href")?.slice(1);
      link.classList.toggle("active", target === activeId);
    }
  }

  function filterDocs() {
    const q = (searchInput.value || "").trim().toLowerCase();
    for (const section of sections) {
      const text = (section.dataset.search + " " + section.textContent).toLowerCase();
      section.classList.toggle("hidden", q.length > 0 && !text.includes(q));
    }
    updateActiveNav();
  }

  searchInput.addEventListener("input", filterDocs);
  window.addEventListener("scroll", updateActiveNav, { passive: true });

  document.addEventListener("keydown", (event) => {
    if (event.key === "/" && document.activeElement !== searchInput) {
      event.preventDefault();
      searchInput.focus();
    } else if (event.key === "Escape" && document.activeElement === searchInput) {
      searchInput.value = "";
      filterDocs();
      searchInput.blur();
    }
  });

  updateActiveNav();
})();
