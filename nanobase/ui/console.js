/*
 * The console.
 *
 * One page, no framework, no build step. The whole thing is a few
 * hundred lines of plain JavaScript that talks to the same HTTP API the
 * command line talks to — the console has no privileged path into the
 * platform, so anything it can do, a script can do.
 *
 * The API it uses:
 *
 *   POST   /api/auth/login                 sign in
 *   GET    /api/auth/me                    who the token belongs to
 *   GET    /api/apps?search=&page=         the apps, filtered and paged
 *   POST   /api/apps                       create one
 *   GET    /api/apps/{id}                  one app's document
 *   PATCH  /api/apps/{id}                  change part of it
 *   DELETE /api/apps/{id}                  remove it
 *   POST   /api/apps/{id}/db/test          try a database configuration
 *   GET    /api/apps/{id}/db               the collections
 *   GET    /api/apps/{id}/db/{collection}  the rows
 */

(function () {
  "use strict";

  // ------------------------------------------------------------- helpers --

  /**
   * Build an element.
   *
   * `props` may hold `class`, `text`, `html` (never used with user data),
   * `on` (event handlers), `attrs` and anything else that goes on the
   * element. Children are elements or strings.
   */
  function el(tag, props, children) {
    var node = document.createElement(tag);
    props = props || {};
    Object.keys(props).forEach(function (key) {
      var value = props[key];
      if (value === undefined || value === null || value === false) return;
      if (key === "class") node.className = value;
      else if (key === "text") node.textContent = value;
      else if (key === "on") {
        Object.keys(value).forEach(function (event) {
          node.addEventListener(event, value[event]);
        });
      } else if (key === "attrs") {
        Object.keys(value).forEach(function (name) {
          node.setAttribute(name, value[name]);
        });
      } else if (key === "value") node.value = value;
      else if (key === "checked") node.checked = !!value;
      else if (key === "disabled") node.disabled = !!value;
      else node[key] = value;
    });
    (Array.isArray(children) ? children : children ? [children] : []).forEach(
      function (child) {
        if (child === null || child === undefined || child === false) return;
        node.appendChild(
          typeof child === "string" || typeof child === "number"
            ? document.createTextNode(String(child))
            : child
        );
      }
    );
    return node;
  }

  function clear(node) {
    while (node.firstChild) node.removeChild(node.firstChild);
    return node;
  }

  /** An inline SVG icon from the sprite the shell uses. */
  function icon(paths, size) {
    var svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("aria-hidden", "true");
    if (size) {
      svg.style.width = size + "px";
      svg.style.height = size + "px";
    }
    paths.forEach(function (d) {
      var node = document.createElementNS("http://www.w3.org/2000/svg", "path");
      node.setAttribute("d", d);
      svg.appendChild(node);
    });
    return svg;
  }

  var ICONS = {
    plus: ["M12 5v14M5 12h14"],
    trash: ["M4 7h16", "M9 7V5h6v2", "M6 7l1 13h10l1-13"],
    check: ["M4 12l5 5L20 6"],
    bolt: ["M13 3 5 14h6l-1 7 8-11h-6z"],
    copy: ["M9 9h10v10H9z", "M5 15V5h10"],
    chevronLeft: ["M15 6l-6 6 6 6"],
    chevronRight: ["M9 6l6 6-6 6"],
  };

  // --------------------------------------------------------------- state --

  var state = {
    token: null,
    user: null,
    route: { name: "apps", key: null, tab: "overview" },
    list: { search: "", page: 1, perPage: 12, total: 0, pages: 1 },
    apps: [],
    keys: [],
    keyApp: "",
    keyApps: [],
  };

  var dom = {};

  // ----------------------------------------------------------------- api --

  function api(method, path, body) {
    var headers = {};
    if (body !== undefined) headers["content-type"] = "application/json";
    if (state.token) headers["authorization"] = "Bearer " + state.token;
    return fetch(path, {
      method: method,
      headers: headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    }).then(function (response) {
      var isJson = (response.headers.get("content-type") || "").indexOf("json") >= 0;
      return (isJson ? response.json() : response.text()).then(function (payload) {
        if (!response.ok) {
          var message =
            payload && payload.message
              ? payload.message
              : "the server said " + response.status;
          var error = new Error(message);
          error.status = response.status;
          error.code = payload && payload.code;
          throw error;
        }
        return payload;
      });
    });
  }

  function toast(message, bad) {
    dom.toast.textContent = message;
    dom.toast.className = "toast" + (bad ? " bad" : "");
    dom.toast.hidden = false;
    clearTimeout(toast.timer);
    toast.timer = setTimeout(function () {
      dom.toast.hidden = true;
    }, bad ? 6000 : 3000);
  }

  function fail(error) {
    toast(error && error.message ? error.message : String(error), true);
  }

  // --------------------------------------------------------------- login --

  function showLogin() {
    dom.app.hidden = true;
    dom.login.hidden = false;
    setTimeout(function () {
      dom.loginEmail.focus();
    }, 0);
  }

  function showConsole() {
    dom.login.hidden = true;
    dom.app.hidden = false;
    dom.who.textContent = state.user ? state.user.email : "";
    render();
  }

  function signIn(event) {
    event.preventDefault();
    dom.loginError.hidden = true;
    dom.loginSubmit.disabled = true;
    api("POST", "/api/auth/login", {
      email: dom.loginEmail.value.trim(),
      password: dom.loginPassword.value,
    })
      .then(function (result) {
        state.token = result.token;
        state.user = result.user;
        try {
          localStorage.setItem("nanobase.token", state.token);
        } catch (e) {
          /* a private window: the token lasts until the tab closes */
        }
        dom.loginPassword.value = "";
        showConsole();
      })
      .catch(function (error) {
        dom.loginError.textContent = error.message;
        dom.loginError.hidden = false;
      })
      .finally(function () {
        dom.loginSubmit.disabled = false;
      });
  }

  function signOut() {
    state.token = null;
    state.user = null;
    try {
      localStorage.removeItem("nanobase.token");
    } catch (e) {
      /* nothing to remove */
    }
    location.hash = "#apps";
    showLogin();
  }

  // -------------------------------------------------------------- router --

  function parseRoute() {
    var hash = (location.hash || "#apps").replace(/^#/, "");
    var parts = hash.split("/").filter(Boolean);
    if (parts[0] === "app" && parts[1]) {
      var tab = parts[2] || "overview";
      return { name: "app", key: decodeURIComponent(parts[1]), tab: tab };
    }
    if (parts[0] === "keys") {
      return { name: "keys", key: null, tab: "overview" };
    }
    return { name: "apps", key: null, tab: "overview" };
  }

  function navigate(hash) {
    if (location.hash === hash) render();
    else location.hash = hash;
  }

  function onRouteChanged() {
    state.route = parseRoute();
    closeNav();
    markNav();
    render();
  }

  /** Mark the sidebar item for the page we are on. */
  function markNav() {
    var wanted = state.route.name === "app" ? "apps" : state.route.name;
    Array.prototype.forEach.call(
      document.querySelectorAll(".nav-item[data-route]"),
      function (item) {
        item.classList.toggle("active", item.getAttribute("data-route") === wanted);
      }
    );
  }

  // ---------------------------------------------------------------- keys --

  var SCOPES = [
    ["rows:read", "Read the rows of an app"],
    ["rows:write", "Write the rows of an app"],
    ["apps:read", "Read the apps"],
    ["apps:write", "Create, change and remove apps"],
    ["admin", "Everything, including making more keys"],
  ];

  function renderKeys(panel) {
    panel.appendChild(
      el("div", { class: "page-head" }, [
        el("div", {}, [
          el("h1", { text: "API keys" }),
          el("p", {
            text: "A credential a program holds, instead of the owner's password.",
          }),
        ]),
        el("div", { class: "spacer" }),
        el("button", {
          class: "button primary",
          type: "button",
          on: { click: openCreateKeyDialog },
        }, [icon(ICONS.plus), "New key"]),
      ])
    );

    panel.appendChild(
      el("div", { class: "section" }, [
        el("p", {
          class: "section-note",
          text:
            "A key belongs to one app: it may only see that app, it may be " +
            "limited to what it may do, and it may be made to expire on its " +
            "own. The key itself is shown once, when it is made.",
        }),
        el("label", { class: "field" }, [
          el("span", { text: "App" }),
          el("select", {
            id: "key-app",
            on: {
              change: function (event) {
                state.keyApp = event.target.value;
                loadKeys();
              },
            },
          }),
        ]),
        el("div", { id: "key-list" }, [el("p", { class: "muted small", text: "Loading…" })]),
      ])
    );

    // The apps a key may belong to.
    api("GET", "/api/apps?perPage=200")
      .then(function (result) {
        state.keyApps = result.items || [];
        var picker = document.getElementById("key-app");
        if (!picker) return;
        clear(picker);
        if (!state.keyApps.length) {
          picker.appendChild(
            el("option", { value: "", text: "no apps yet" })
          );
          state.keyApp = "";
          clear(document.getElementById("key-list")).appendChild(
            el("p", {
              class: "muted small",
              text: "Make an app first: a key belongs to one.",
            })
          );
          return;
        }
        state.keyApps.forEach(function (item) {
          var label =
            (item.doc && item.doc.app && item.doc.app.name) || item.id;
          picker.appendChild(el("option", { value: item.id, text: label }));
        });
        if (!state.keyApp) {
          state.keyApp = state.keyApps[0].id;
        }
        picker.value = state.keyApp;
        loadKeys();
      })
      .catch(function (error) {
        clear(document.getElementById("key-list")).appendChild(
          el("p", { class: "error", text: error.message })
        );
      });
  }

  /** The keys of the app that is being looked at. */
  function loadKeys() {
    var box = document.getElementById("key-list");
    if (!box || !state.keyApp) return;
    api("GET", "/api/apps/" + encodeURIComponent(state.keyApp) + "/keys")
      .then(function (result) {
        state.keys = result.items || [];
        drawKeys();
      })
      .catch(function (error) {
        clear(document.getElementById("key-list")).appendChild(
          el("p", { class: "error", text: error.message })
        );
      });
  }

  function drawKeys() {
    var box = clear(document.getElementById("key-list"));
    if (!state.keys.length) {
      box.appendChild(
        el("p", {
          class: "muted small",
          text: "No keys yet. Make one for the programs that talk to this app.",
        })
      );
      return;
    }
    state.keys.forEach(function (key) {
      var expired = key.expiresAt && new Date(key.expiresAt) < new Date();
      box.appendChild(
        el("div", { class: "section", style: "background:var(--surface-2);padding:14px" }, [
          el("div", { class: "row" }, [
            el("strong", { text: key.name }),
            el("span", { class: "mono small muted", text: key.preview }),
            el("span", { class: "spacer", style: "flex:1 1 auto" }),
            key.revoked
              ? el("span", { class: "tag danger", text: "revoked" })
              : expired
              ? el("span", { class: "tag warn", text: "expired" })
              : el("span", { class: "tag ok", text: "active" }),
            el("button", {
              class: "button small ghost",
              type: "button",
              title: "Remove",
              on: { click: function () { removeKey(key); } },
            }, [icon(ICONS.trash)]),
          ]),
          el("p", {
            class: "mono small muted",
            text: (key.scopes || []).join("  ") || "no scopes",
          }),
          el("p", {
            class: "small muted",
            text:
              "expires: " + (key.expiresAt || "never") +
              " · last used: " + (key.lastUsedAt || "never"),
          }),
        ])
      );
    });
  }

  function removeKey(key) {
    if (!window.confirm('Remove the key "' + key.name + '"?')) return;
    var app = key.app || state.keyApp;
    api("DELETE", "/api/apps/" + encodeURIComponent(app) + "/keys/" + encodeURIComponent(key.id))
      .then(function () {
        toast("The key is removed.");
        loadKeys();
      })
      .catch(function (error) { toast(error.message, true); });
  }

  function openCreateKeyDialog() {
    var name = el("input", { type: "text", placeholder: "the deploy script" });
    var app = el("select", {}, []);
    var expires = el("select", {}, [
      el("option", { value: "30", text: "in 30 days" }),
      el("option", { value: "7", text: "in 7 days" }),
      el("option", { value: "1", text: "in a day" }),
      el("option", { value: "0", text: "never" }),
    ]);
    var checks = SCOPES.map(function (scope) {
      var box = el("input", { type: "checkbox", checked: scope[0] === "rows:read" });
      return { scope: scope[0], box: box, label: scope[1] };
    });

    var dialog = el("div", { class: "dialog" }, [
      el("h2", { text: "New API key" }),
      el("label", { class: "field" }, [el("span", { text: "Name" }), name]),
      el("label", { class: "field" }, [el("span", { text: "App" }), app]),
      el("label", { class: "field" }, [el("span", { text: "Expires" }), expires]),
      el("div", { class: "field" }, [
        el("span", { text: "Scopes" }),
        checks.map(function (entry) {
          return el("label", { class: "switch" }, [
            entry.box,
            el("span", { text: entry.label }),
          ]);
        }),
      ]),
      el("p", {
        class: "muted small",
        text: "The key is shown once, when it is made. Put it in a secret store.",
      }),
      el("div", { class: "row" }, [
        el("button", {
          class: "button primary",
          type: "button",
          text: "Create",
          on: {
            click: function () {
              var scopes = checks
                .filter(function (entry) { return entry.box.checked; })
                .map(function (entry) { return entry.scope; });
              if (!scopes.length) {
                toast("A key needs at least one scope", true);
                return;
              }
              var days = parseInt(expires.value, 10) || 0;
              var body = { name: name.value.trim(), scopes: scopes };
              if (days > 0) {
                body.expiresAt = Math.floor(Date.now() / 1000) + days * 86400;
              }
              if (!app.value) {
                toast("A key belongs to an app: choose one", true);
                return;
              }
              api(
                "POST",
                "/api/apps/" + encodeURIComponent(app.value) + "/keys",
                body
              )
                .then(function (result) {
                  closeDialog();
                  showSecret(result.secret);
                  state.keyApp = app.value;
                  loadKeys();
                })
                .catch(function (error) { toast(error.message, true); });
            },
          },
        }),
        el("button", {
          class: "button",
          type: "button",
          text: "Cancel",
          on: { click: closeDialog },
        }),
      ]),
    ]);

    var backdrop = el("div", { class: "dialog-backdrop" }, [dialog]);
    backdrop.addEventListener("click", function (event) {
      if (event.target === backdrop) closeDialog();
    });
    document.body.appendChild(backdrop);

    // The apps the key may belong to.
    api("GET", "/api/apps?perPage=200")
      .then(function (result) {
        (result.items || []).forEach(function (item) {
          var label =
            (item.doc && item.doc.app && item.doc.app.name) || item.id;
          app.appendChild(el("option", { value: item.id, text: label }));
        });
        if (state.keyApp) app.value = state.keyApp;
      })
      .catch(function () { /* the list is a convenience, not a requirement */ });
  }

  function showSecret(secret) {
    var field = el("input", { type: "text", value: secret, readonly: true });
    var backdrop = el("div", { class: "dialog-backdrop" }, [
      el("div", { class: "dialog" }, [
        el("h2", { text: "Your new key" }),
        el("p", {
          class: "section-note",
          text: "This is the only time it is shown. Copy it now.",
        }),
        field,
        el("div", { class: "row" }, [
          el("button", {
            class: "button primary",
            type: "button",
            text: "Copy",
            on: {
              click: function () {
                if (navigator.clipboard) navigator.clipboard.writeText(secret);
                field.select();
                toast("Copied.");
              },
            },
          }),
          el("button", {
            class: "button",
            type: "button",
            text: "Done",
            on: { click: closeDialog },
          }),
        ]),
      ]),
    ]);
    backdrop.addEventListener("click", function (event) {
      if (event.target === backdrop) closeDialog();
    });
    document.body.appendChild(backdrop);
    field.select();
  }

  // ----------------------------------------------------------- app list --

  function loadApps() {
    var query = new URLSearchParams();
    if (state.list.search) query.set("search", state.list.search);
    query.set("page", String(state.list.page));
    query.set("perPage", String(state.list.perPage));
    return api("GET", "/api/apps?" + query.toString()).then(function (result) {
      state.apps = result.items || [];
      state.list.total = result.total || 0;
      state.list.pages = result.pages || 1;
    });
  }

  function renderApps(panel) {
    var head = el("div", { class: "page-head" }, [
      el("div", {}, [
        el("h1", { text: "Apps" }),
        el("p", {
          text: state.list.total
            ? state.list.total + (state.list.total === 1 ? " app" : " apps")
            : "No apps yet",
        }),
      ]),
      el("div", { class: "spacer" }),
      el("button", {
        class: "button primary",
        type: "button",
        on: { click: openCreateDialog },
      }, [icon(ICONS.plus), "New app"]),
    ]);
    panel.appendChild(head);

    if (state.apps.length === 0) {
      var emptyText = state.list.search
        ? "Nothing matches “" + state.list.search + "”."
        : "Create your first app to give it a database, an auth setup and a set of rules.";
      panel.appendChild(
        el("div", { class: "empty" }, [
          el("h3", { text: state.list.search ? "No matches" : "No apps yet" }),
          el("p", { text: emptyText }),
          state.list.search
            ? null
            : el("button", {
                class: "button primary",
                type: "button",
                on: { click: openCreateDialog },
              }, [icon(ICONS.plus), "New app"]),
        ])
      );
      return;
    }

    var grid = el("div", { class: "app-grid" });
    state.apps.forEach(function (app) {
      grid.appendChild(appCard(app));
    });
    panel.appendChild(grid);
    panel.appendChild(pager());
  }

  function appCard(app) {
    var doc = app.doc || {};
    var config = doc.config || {};
    var database = config.database || {};
    var auth = config.auth || {};
    var acl = config.acl || {};
    var name = (doc.app && doc.app.name) || "Untitled";
    var href = "#app/" + encodeURIComponent(app.id);

    return el("div", { class: "app-card" }, [
      el("div", { class: "app-card-head" }, [
        el("div", { style: "min-width:0" }, [
          el("h3", { text: name }),
          el("div", { class: "id", text: app.id }),
        ]),
      ]),
      el("div", { class: "row" }, [
        el("span", {
          class: "tag accent",
          text: database.provider
            ? (database.type || "sqlite") + " \u00b7 " + database.provider
            : database.type || "sqlite",
        }),
        auth.features && auth.features.emailPassword
          ? el("span", { class: "tag ok", text: "auth on" })
          : el("span", { class: "tag", text: "no auth" }),
        acl.enabled ? el("span", { class: "tag warn", text: "acl on" }) : null,
      ]),
      el("div", { class: "app-card-foot" }, [
        el("a", { class: "button small", href: href, text: "Open" }),
        el("span", { class: "spacer", style: "flex:1 1 auto" }),
        el("button", {
          class: "button small ghost",
          type: "button",
          title: "Copy the id",
          on: {
            click: function () {
              navigator.clipboard
                ? navigator.clipboard.writeText(app.id)
                : toast("The id is " + app.id);
              toast("Copied " + app.id);
            },
          },
        }, [icon(ICONS.copy)]),
      ]),
    ]);
  }

  function pager() {
    var list = state.list;
    var from = list.total === 0 ? 0 : (list.page - 1) * list.perPage + 1;
    var to = Math.min(list.page * list.perPage, list.total);
    return el("div", { class: "pager" }, [
      el("span", {
        class: "count",
        text: list.total === 0 ? "No apps" : from + "–" + to + " of " + list.total,
      }),
      el("span", { class: "spacer" }),
      el("button", {
        class: "button small",
        type: "button",
        disabled: list.page <= 1,
        text: "Previous",
        on: {
          click: function () {
            state.list.page = Math.max(1, list.page - 1);
            render();
          },
        },
      }),
      el("span", { class: "count", text: "Page " + list.page + " of " + list.pages }),
      el("button", {
        class: "button small",
        type: "button",
        disabled: list.page >= list.pages,
        text: "Next",
        on: {
          click: function () {
            state.list.page = Math.min(list.pages, list.page + 1);
            render();
          },
        },
      }),
    ]);
  }

  // ------------------------------------------------------- create dialog --

  function openCreateDialog() {
    var name = el("input", { type: "text", placeholder: "todos", attrs: { maxlength: "60" } });
    var error = el("p", { class: "error", hidden: true });
    var submit = el("button", { class: "button primary", type: "submit", text: "Create" });

    var form = el("form", { class: "dialog", on: { submit: create } }, [
      el("h2", { text: "New app" }),
      el("p", {
        text: "An app gets its own database, its own auth setup and its own rules.",
      }),
      el("label", { class: "field" }, [
        el("span", { text: "Name" }),
        name,
        el("span", {
          class: "hint",
          text: "Letters, digits, spaces, - and _. The name is also its id in the API.",
        }),
      ]),
      error,
      el("div", { class: "dialog-actions" }, [
        el("button", {
          class: "button",
          type: "button",
          text: "Cancel",
          on: { click: closeDialog },
        }),
        submit,
      ]),
    ]);

    var backdrop = el("div", { class: "dialog-backdrop", on: { click: backdropClick } }, [
      form,
    ]);
    document.body.appendChild(backdrop);
    setTimeout(function () {
      name.focus();
    }, 0);

    function backdropClick(event) {
      if (event.target === backdrop) closeDialog();
    }

    function create(event) {
      event.preventDefault();
      error.hidden = true;
      submit.disabled = true;
      api("POST", "/api/apps", { name: name.value.trim() })
        .then(function (app) {
          closeDialog();
          toast("Created " + (app.doc && app.doc.app ? app.doc.app.name : "the app"));
          navigate("#app/" + encodeURIComponent(app.id));
        })
        .catch(function (problem) {
          error.textContent = problem.message;
          error.hidden = false;
        })
        .finally(function () {
          submit.disabled = false;
        });
    }
  }

  function closeDialog() {
    var open = document.querySelector(".dialog-backdrop");
    if (open) open.remove();
  }

  // --------------------------------------------------------- app detail --

  var TABS = [
    { id: "overview", label: "Overview" },
    { id: "database", label: "Database" },
    { id: "auth", label: "Auth" },
    { id: "cors", label: "CORS" },
    { id: "acl", label: "Access" },
    { id: "rows", label: "Rows" },
  ];

  function renderApp(panel, app) {
    var doc = app.doc || {};
    var name = (doc.app && doc.app.name) || "Untitled";
    panel.appendChild(
      el("div", { class: "breadcrumb" }, [
        el("a", { href: "#apps", text: "Apps" }),
        el("span", { text: "/" }),
        el("span", { text: name }),
      ])
    );
    panel.appendChild(
      el("div", { class: "page-head" }, [
        el("div", {}, [
          el("h1", { text: name }),
          el("p", { class: "mono", text: app.id }),
        ]),
        el("div", { class: "spacer" }),
        el("button", {
          class: "button danger",
          type: "button",
          on: { click: function () { openDeleteDialog(app, name); } },
        }, [icon(ICONS.trash), "Delete"]),
      ])
    );

    var tabs = el("div", { class: "tabs" });
    TABS.forEach(function (tab) {
      tabs.appendChild(
        el("button", {
          class: "tab" + (state.route.tab === tab.id ? " active" : ""),
          type: "button",
          text: tab.label,
          on: {
            click: function () {
              navigate("#app/" + encodeURIComponent(app.id) + "/" + tab.id);
            },
          },
        })
      );
    });
    panel.appendChild(tabs);

    var body = el("div", {});
    panel.appendChild(body);
    switch (state.route.tab) {
      case "database":
        renderDatabaseTab(body, app);
        break;
      case "auth":
        renderAuthTab(body, app);
        break;
      case "cors":
        renderCorsTab(body, app);
        break;
      case "acl":
        renderAclTab(body, app);
        break;
      case "rows":
        renderRowsTab(body, app);
        break;
      default:
        renderOverviewTab(body, app);
    }
  }

  function renderOverviewTab(panel, app) {
    var doc = app.doc || {};
    var config = doc.config || {};
    var schema = doc.schema || {};
    var collections = Object.keys(schema);

    var kv = el("dl", { class: "kv" });
    [
      ["Id", app.id],
      ["Name", (doc.app && doc.app.name) || ""],
      ["Type", (doc.app && doc.app.type) || "local"],
      ["Created", app.createdAt],
      ["Database", (config.database && config.database.type) || "sqlite"],
      [
        "Storage",
        (config.storage && config.storage.type) || "local",
      ],
      [
        "Auth",
        config.auth && config.auth.features && config.auth.features.emailPassword
          ? "email and password"
          : "off",
      ],
      [
        "CORS",
        config.cors && config.cors.enabled
          ? (config.cors.origins || []).join(", ") || "on, every origin"
          : "off",
      ],
      ["Access rules", config.acl && config.acl.enabled ? "on" : "off"],
      ["Collections", collections.length ? collections.join(", ") : "none"],
    ].forEach(function (pair) {
      kv.appendChild(el("dt", { text: pair[0] }));
      kv.appendChild(el("dd", { class: "mono", text: String(pair[1]) }));
    });

    panel.appendChild(
      el("div", { class: "section" }, [
        el("h2", { text: "Overview" }),
        el("p", {
          class: "section-note",
          text: "What this app is, as the platform sees it.",
        }),
        kv,
      ])
    );
  }

  // -- the database tab ---------------------------------------------------

  function renderDatabaseTab(panel, app) {
    var config = (app.doc && app.doc.config) || {};
    var database = config.database || {};
    // The platform does not hand out the app's secrets, so the fields
    // that hold one are named beside the document. A field that is not
    // changed is not sent: the platform keeps what it has.
    var secrets = app.secrets || [];
    var hasToken = secrets.indexOf("config.database.token") >= 0;
    var hasUrlSecret = secrets.indexOf("config.database.url") >= 0;

    var type = el("select", {}, []);
    [
      ["sqlite", "SQLite — a local file"],
      ["memory", "Memory — in-process, gone on restart"],
      ["libsql", "libSQL / Turso — SQLite over HTTP"],
      ["d1", "Cloudflare D1 — over HTTP"],
      ["postgres", "PostgreSQL — needs a driver"],
      ["mysql", "MySQL — needs a driver"],
    ].forEach(function (pair) {
      type.appendChild(
        el("option", {
          value: pair[0],
          text: pair[1],
          attrs: { selected: (database.type || "sqlite") === pair[0] ? "selected" : null },
        })
      );
    });

    // Which cloud the rows are in. Left alone, the platform reads it
    // out of the URL — a URL says a protocol more often than it says a
    // cloud. Named here, the name is what the platform uses: a Neon
    // database behind a console URL, a TiDB Cloud behind a proxy, a
    // MySQL host that is neither TiDB nor PlanetScale.
    var clouds = [
      ["", "Automatic \u2014 read from the URL"],
      ["turso", "Turso \u2014 libSQL over HTTP"],
      ["d1", "Cloudflare D1"],
      ["neon", "Neon \u2014 PostgreSQL over HTTP"],
      ["tidb", "TiDB Cloud \u2014 MySQL over HTTP"],
      ["planetscale", "PlanetScale \u2014 MySQL over HTTP"],
    ];
    // The engine each cloud speaks, so that the two never disagree.
    var engineOf = {
      turso: "libsql",
      d1: "d1",
      neon: "postgres",
      tidb: "mysql",
      planetscale: "mysql",
    };
    var provider = el("select", {}, []);
    clouds.forEach(function (pair) {
      provider.appendChild(
        el("option", {
          value: pair[0],
          text: pair[1],
          attrs: {
            selected: (database.provider || "") === pair[0] ? "selected" : null,
          },
        })
      );
    });

    var url = el("input", {
      type: "text",
      value: database.url || "",
      placeholder: "http://127.0.0.1:8091",
    });
    var token = el("input", {
      type: "text",
      value: database.token || "",
      placeholder: "the bearer token, if the database wants one",
    });
    var result = el("div", {});

    // What the fields said when the page was opened. A field that has
    // not moved is not sent, so a secret the platform holds is not
    // wiped by a save that did not mean to touch it.
    var wasUrl = url.value;
    var wasToken = token.value;
    var wasProvider = provider.value;

    // Choosing a cloud chooses the engine it speaks, so that the two
    // never say different things about the same database.
    provider.addEventListener("change", function () {
      var engine = engineOf[provider.value];
      if (engine) type.value = engine;
    });

    function candidate() {
      var changes = { type: type.value };
      if (url.value !== wasUrl) changes.url = url.value.trim() || null;
      if (token.value !== wasToken) changes.token = token.value.trim() || null;
      if (provider.value !== wasProvider) {
        changes.provider = provider.value || null;
      }
      return changes;
    }

    function test() {
      var box = clear(result);
      box.appendChild(el("p", { class: "muted small", text: "Testing…" }));
      api("POST", "/api/apps/" + encodeURIComponent(app.id) + "/db/test", {
        database: candidate(),
      })
        .then(function (answer) {
          var box = clear(result);
          if (answer.ok) {
            box.appendChild(
              el("p", { class: "notice" }, [
                el("span", { class: "dot" }),
                " Connected. " +
                  (answer.collections && answer.collections.length
                    ? "Collections: " + answer.collections.join(", ")
                    : "No collections yet."),
              ])
            );
          } else {
            box.appendChild(
              el("p", { class: "error", text: "Not connected: " + answer.error })
            );
          }
        })
        .catch(function (error) {
          clear(result).appendChild(el("p", { class: "error", text: error.message }));
        });
    }

    panel.appendChild(
      el("div", { class: "section" }, [
        el("h2", { text: "Database" }),
        el("p", {
          class: "section-note",
          text: "Where this app's rows live. Test the connection before you save it.",
        }),
        el("label", { class: "field" }, [el("span", { text: "Engine" }), type]),
        el("label", { class: "field" }, [
          el("span", { text: "Cloud" }),
          provider,
          el("span", {
            class: "hint",
            text:
              "Which cloud the rows are in. Leave it on automatic and the platform reads it from the URL; name it when the URL cannot say \u2014 a console URL, a proxy, or a MySQL host that is neither TiDB nor PlanetScale.",
          }),
        ]),
        el("label", { class: "field" }, [
          el("span", { text: "URL" }),
          url,
          el("span", {
            class: "hint",
            text: hasUrlSecret
              ? "The credentials in this URL are not shown. Leave it alone to keep them; type a whole URL to replace it."
              : "A local file needs no URL. A remote engine needs one.",
          }),
        ]),
        el("label", { class: "field" }, [
          el("span", { text: "Token" }),
          token,
          el("span", {
            class: "hint",
            text: hasToken
              ? "A token is stored with this app. It is not shown. Type here to replace it."
              : "Stored with the app. Keep it secret.",
          }),
        ]),
        el("div", { class: "row" }, [
          el("button", {
            class: "button",
            type: "button",
            text: "Test connection",
            on: { click: test },
          }),
          el("button", {
            class: "button primary",
            type: "button",
            text: "Save",
            on: {
              click: function () {
                patchApp(
                  app,
                  { config: { database: candidate() } },
                  "The database is saved."
                );
              },
            },
          }),
        ]),
        result,
      ])
    );
  }

  // -- the auth tab -------------------------------------------------------

  function renderAuthTab(panel, app) {
    var config = (app.doc && app.doc.config) || {};
    var auth = config.auth || {};
    var features = auth.features || {};
    var providers = auth.providers || {};

    var emailPassword = el("input", {
      type: "checkbox",
      checked: !!features.emailPassword,
    });

    var providerRows = Object.keys(providers).map(function (name) {
      var provider = providers[name] || {};
      return el("div", { class: "field" }, [
        el("span", { text: name }),
        el("input", {
          type: "text",
          value: provider.clientId || "",
          placeholder: "client id",
        }),
        el("input", {
          type: "text",
          value: provider.clientSecret || "",
          placeholder: "client secret",
        }),
      ]);
    });

    panel.appendChild(
      el("div", { class: "section" }, [
        el("h2", { text: "Authentication" }),
        el("p", {
          class: "section-note",
          text: "How people sign in to this app.",
        }),
        el("label", { class: "switch" }, [
          emailPassword,
          el("span", { text: "Email and password" }),
        ]),
        providerRows.length
          ? el("div", {}, [
              el("h2", { text: "Providers", style: "font-size:14px;margin:18px 0 10px" }),
            ].concat(providerRows))
          : el("p", {
              class: "muted small",
              text: "No identity providers are configured for this app.",
            }),
        el("div", { class: "row" }, [
          el("button", {
            class: "button primary",
            type: "button",
            text: "Save",
            on: {
              click: function () {
                patchApp(
                  app,
                  {
                    config: {
                      auth: {
                        features: { emailPassword: emailPassword.checked },
                      },
                    },
                  },
                  "The auth settings are saved."
                );
              },
            },
          }),
        ]),
      ])
    );
  }

  // -- the cors tab -------------------------------------------------------

  function renderCorsTab(panel, app) {
    var config = (app.doc && app.doc.config) || {};
    var cors = config.cors || {};

    var enabled = el("input", { type: "checkbox", checked: !!cors.enabled });
    var origins = el("input", {
      type: "text",
      value: (cors.origins || []).join(", "),
      placeholder: "http://localhost:3000, https://example.com",
    });
    var credentials = el("input", {
      type: "checkbox",
      checked: !!cors.allowCredentials,
    });
    var maxAge = el("input", {
      type: "number",
      value: String(cors.maxAge === undefined ? 600 : cors.maxAge),
      min: "0",
    });

    panel.appendChild(
      el("div", { class: "section" }, [
        el("h2", { text: "CORS" }),
        el("p", {
          class: "section-note",
          text:
            "Which browser origins may read this app's answers. An origin " +
            "that is not on the list gets no allow header, and the browser " +
            "refuses to hand it the answer.",
        }),
        el("label", { class: "switch" }, [
          enabled,
          el("span", { text: "Turn CORS on for this app" }),
        ]),
        el("label", { class: "field" }, [
          el("span", { text: "Origins" }),
          origins,
          el("span", {
            class: "hint",
            text:
              "Comma separated. A full origin: scheme, host and port. " +
              "A single * allows every origin.",
          }),
        ]),
        el("label", { class: "switch" }, [
          credentials,
          el("span", { text: "Allow credentials (cookies, authorization)" }),
        ]),
        el("label", { class: "field" }, [
          el("span", { text: "Preflight cache (seconds)" }),
          maxAge,
          el("span", {
            class: "hint",
            text: "How long a browser may keep the preflight answer.",
          }),
        ]),
        el("p", {
          class: "muted small",
          text:
            "An app with CORS of its own uses it. An app without one " +
            "inherits the platform's list, set when the server was started.",
        }),
        el("div", { class: "row" }, [
          el("button", {
            class: "button primary",
            type: "button",
            text: "Save",
            on: {
              click: function () {
                var list = origins.value
                  .split(",")
                  .map(function (origin) { return origin.trim(); })
                  .filter(Boolean);
                patchApp(
                  app,
                  {
                    config: {
                      cors: {
                        enabled: enabled.checked,
                        origins: list,
                        allowCredentials: credentials.checked,
                        maxAge: parseInt(maxAge.value, 10) || 0,
                      },
                    },
                  },
                  "The CORS settings are saved."
                );
              },
            },
          }),
        ]),
      ])
    );
  }

  // -- the access tab -----------------------------------------------------

  function renderAclTab(panel, app) {
    var config = (app.doc && app.doc.config) || {};
    var acl = config.acl || {};
    var rules = acl.rules || [];

    var enabled = el("input", { type: "checkbox", checked: !!acl.enabled });
    var list = el("div", {});

    function drawRules(current) {
      var box = clear(list);
      if (!current.length) {
        box.appendChild(
          el("p", {
            class: "muted small",
            text: "No rules. With access control on and no rules, nothing is permitted.",
          })
        );
        return;
      }
      current.forEach(function (rule, index) {
        box.appendChild(
          el("div", { class: "section", style: "background:var(--surface-2);padding:14px" }, [
            el("div", { class: "row" }, [
              el("strong", { text: rule.name || rule.id || "Rule " + (index + 1) }),
              el("span", {
                class: "tag " + (rule.effect === "deny" ? "danger" : "ok"),
                text: rule.effect || "allow",
              }),
              el("span", { class: "spacer", style: "flex:1 1 auto" }),
              el("button", {
                class: "button small ghost",
                type: "button",
                title: "Remove",
                on: {
                  click: function () {
                    var next = current.slice();
                    next.splice(index, 1);
                    drawRules(next);
                  },
                },
              }, [icon(ICONS.trash)]),
            ]),
            el("p", {
              class: "mono small muted",
              text: (rule.permissions || []).join("  "),
            }),
            rule.check
              ? el("pre", {
                  class: "mono small",
                  style: "overflow:auto;margin:8px 0 0",
                  text: JSON.stringify(rule.check, null, 2),
                })
              : null,
          ])
        );
      });
    }
    drawRules(rules);

    var id = el("input", { type: "text", placeholder: "rule id" });
    var name = el("input", { type: "text", placeholder: "rule name" });
    var permissions = el("input", {
      type: "text",
      placeholder: "collection:action:resource",
    });
    var effect = el("select", {}, [
      el("option", { value: "allow", text: "allow" }),
      el("option", { value: "deny", text: "deny" }),
    ]);
    var check = el("textarea", { placeholder: '{ "owner": { "$eq": "$auth.id" } }' });

    function addRule() {
      var rule = {
        id: id.value.trim() || "rule-" + Date.now(),
        permissions: permissions.value
          .split(/[\s,]+/)
          .filter(Boolean),
        effect: effect.value,
      };
      if (name.value.trim()) rule.name = name.value.trim();
      if (check.value.trim()) {
        try {
          rule.check = JSON.parse(check.value);
        } catch (e) {
          toast("The condition is not JSON", true);
          return;
        }
      }
      if (!rule.permissions.length) {
        toast("A rule needs at least one permission", true);
        return;
      }
      var next = rules.slice();
      next.push(rule);
      drawRules(next);
      rules = next;
      id.value = "";
      name.value = "";
      permissions.value = "";
      check.value = "";
    }

    panel.appendChild(
      el("div", { class: "section" }, [
        el("h2", { text: "Access" }),
        el("p", {
          class: "section-note",
          text: "Who may do what. A permission is collection:action:resource, and * matches a part.",
        }),
        el("label", { class: "switch" }, [
          enabled,
          el("span", { text: "Turn access control on" }),
        ]),
        list,
        el("h2", { text: "Add a rule", style: "font-size:14px;margin:20px 0 10px" }),
        el("label", { class: "field" }, [el("span", { text: "Id" }), id]),
        el("label", { class: "field" }, [el("span", { text: "Name" }), name]),
        el("label", { class: "field" }, [
          el("span", { text: "Permissions" }),
          permissions,
          el("span", { class: "hint", text: "For example: todos:list:* or *:read:*" }),
        ]),
        el("label", { class: "field" }, [el("span", { text: "Effect" }), effect]),
        el("label", { class: "field" }, [
          el("span", { text: "Condition (JSON, optional)" }),
          check,
        ]),
        el("div", { class: "row" }, [
          el("button", { class: "button", type: "button", text: "Add rule", on: { click: addRule } }),
          el("button", {
            class: "button primary",
            type: "button",
            text: "Save",
            on: {
              click: function () {
                patchApp(
                  app,
                  { config: { acl: { enabled: enabled.checked, rules: rules } } },
                  "The rules are saved."
                );
              },
            },
          }),
        ]),
      ])
    );
  }

  // -- the rows tab -------------------------------------------------------

  function renderRowsTab(panel, app) {
    var body = el("div", {});
    panel.appendChild(
      el("div", { class: "section" }, [
        el("h2", { text: "Rows" }),
        el("p", {
          class: "section-note",
          text: "The collections this app has, and the rows in them.",
        }),
        body,
      ])
    );
    api("GET", "/api/apps/" + encodeURIComponent(app.id) + "/db")
      .then(function (result) {
        var names = result.collections || [];
        if (!names.length) {
          clear(body).appendChild(
            el("p", { class: "muted small", text: "No collections yet." })
          );
          return;
        }
        var select = el("select", {}, []);
        names.forEach(function (name) {
          select.appendChild(el("option", { value: name, text: name }));
        });
        var rows = el("div", { style: "margin-top:14px" });
        function load() {
          clear(rows).appendChild(el("p", { class: "muted small", text: "Loading…" }));
          api(
            "GET",
            "/api/apps/" +
              encodeURIComponent(app.id) +
              "/db/" +
              encodeURIComponent(select.value) +
              "?limit=50"
          )
            .then(function (page) {
              var box = clear(rows);
              if (!page.items || !page.items.length) {
                box.appendChild(el("p", { class: "muted small", text: "No rows." }));
                return;
              }
              var table = el("table", { class: "stackable" });
              var head = el("tr", {});
              Object.keys(page.items[0]).forEach(function (key) {
                head.appendChild(el("th", { text: key }));
              });
              table.appendChild(el("thead", {}, [head]));
              var tbody = el("tbody", {});
              page.items.forEach(function (item) {
                var tr = el("tr", {});
                Object.keys(page.items[0]).forEach(function (key) {
                  tr.appendChild(
                    el("td", {
                      class: "mono",
                      text:
                        item[key] === null || item[key] === undefined
                          ? "—"
                          : typeof item[key] === "object"
                          ? JSON.stringify(item[key])
                          : String(item[key]),
                    })
                  );
                });
                tbody.appendChild(tr);
              });
              table.appendChild(tbody);
              box.appendChild(el("div", { class: "table-wrap" }, [table]));
              box.appendChild(
                el("p", {
                  class: "pager count",
                  text:
                    "Showing " +
                    page.items.length +
                    " of " +
                    (page.total || 0) +
                    " rows.",
                })
              );
            })
            .catch(function (error) {
              clear(rows).appendChild(el("p", { class: "error", text: error.message }));
            });
        }
        select.addEventListener("change", load);
        body.appendChild(el("label", { class: "field" }, [
          el("span", { text: "Collection" }),
          select,
        ]));
        body.appendChild(rows);
        load();
      })
      .catch(function (error) {
        clear(body).appendChild(el("p", { class: "error", text: error.message }));
      });
  }

  // -- delete dialog ------------------------------------------------------

  function openDeleteDialog(app, name) {
    var confirm = el("input", { type: "text", placeholder: name });
    var error = el("p", { class: "error", hidden: true });
    var submit = el("button", {
      class: "button danger",
      type: "submit",
      text: "Delete",
    });

    var form = el("form", { class: "dialog", on: { submit: remove } }, [
      el("h2", { text: "Delete " + name }),
      el("p", {
        text: "The app and its document go. Type the name to confirm.",
      }),
      el("label", { class: "field" }, [el("span", { text: "Name" }), confirm]),
      error,
      el("div", { class: "dialog-actions" }, [
        el("button", {
          class: "button",
          type: "button",
          text: "Cancel",
          on: { click: closeDialog },
        }),
        submit,
      ]),
    ]);
    document.body.appendChild(el("div", { class: "dialog-backdrop" }, [form]));
    setTimeout(function () {
      confirm.focus();
    }, 0);

    function remove(event) {
      event.preventDefault();
      error.hidden = true;
      submit.disabled = true;
      api("DELETE", "/api/apps/" + encodeURIComponent(app.id), {
        confirm: confirm.value.trim(),
      })
        .then(function () {
          closeDialog();
          toast("Deleted " + name);
          navigate("#apps");
        })
        .catch(function (problem) {
          error.textContent = problem.message;
          error.hidden = false;
        })
        .finally(function () {
          submit.disabled = false;
        });
    }
  }

  // -- patching an app ----------------------------------------------------

  function patchApp(app, changes, message) {
    api("PATCH", "/api/apps/" + encodeURIComponent(app.id), changes)
      .then(function () {
        toast(message);
        render();
      })
      .catch(fail);
  }

  // --------------------------------------------------------------- render --

  function render() {
    if (!state.token) {
      showLogin();
      return;
    }
    var panel = clear(dom.panel);
    state.route = parseRoute();
    markNav();

    if (state.route.name === "keys") {
      renderKeys(clear(dom.panel));
      return;
    }

    if (state.route.name === "app") {
      api("GET", "/api/apps/" + encodeURIComponent(state.route.key))
        .then(function (app) {
          renderApp(clear(dom.panel), app);
        })
        .catch(function (error) {
          if (error.status === 401) {
            signOut();
            return;
          }
          clear(dom.panel).appendChild(
            el("div", { class: "empty" }, [
              el("h3", { text: "That app is not here" }),
              el("p", { text: error.message }),
              el("a", { class: "button", href: "#apps", text: "Back to the apps" }),
            ])
          );
        });
      return;
    }

    loadApps()
      .then(function () {
        renderApps(clear(dom.panel));
      })
      .catch(function (error) {
        if (error.status === 401) {
          signOut();
          return;
        }
        clear(dom.panel).appendChild(
          el("div", { class: "empty" }, [
            el("h3", { text: "The apps could not be loaded" }),
            el("p", { text: error.message }),
          ])
        );
      });
  }

  // ---------------------------------------------------------------- shell --

  function closeNav() {
    dom.app.classList.remove("nav-open");
    dom.menuToggle.setAttribute("aria-expanded", "false");
  }

  function toggleNav() {
    var open = dom.app.classList.toggle("nav-open");
    dom.menuToggle.setAttribute("aria-expanded", open ? "true" : "false");
  }

  function start() {
    dom.app = document.getElementById("app");
    dom.panel = document.getElementById("panel");
    dom.login = document.getElementById("login");
    dom.loginForm = document.getElementById("login-form");
    dom.loginEmail = document.getElementById("login-email");
    dom.loginPassword = document.getElementById("login-password");
    dom.loginError = document.getElementById("login-error");
    dom.loginSubmit = document.getElementById("login-submit");
    dom.who = document.getElementById("who");
    dom.toast = document.getElementById("toast");
    dom.menuToggle = document.getElementById("menu-toggle");
    dom.version = document.getElementById("version");
    dom.searchInput = document.getElementById("search-input");

    dom.version.textContent = "Nanobase console";

    dom.loginForm.addEventListener("submit", signIn);
    document.getElementById("logout").addEventListener("click", signOut);
    document.getElementById("new-app").addEventListener("click", openCreateDialog);
    dom.menuToggle.addEventListener("click", toggleNav);
    document.getElementById("scrim").addEventListener("click", closeNav);

    var searchTimer = null;
    dom.searchInput.addEventListener("input", function () {
      clearTimeout(searchTimer);
      searchTimer = setTimeout(function () {
        state.list.search = dom.searchInput.value.trim();
        state.list.page = 1;
        if (state.route.name !== "apps") {
          navigate("#apps");
          return;
        }
        render();
      }, 220);
    });

    window.addEventListener("hashchange", onRouteChanged);

    // A token from an earlier session, if there is one.
    var saved = null;
    try {
      saved = localStorage.getItem("nanobase.token");
    } catch (e) {
      saved = null;
    }
    if (saved) {
      state.token = saved;
      api("GET", "/api/auth/me")
        .then(function (result) {
          state.user = result.user;
          showConsole();
        })
        .catch(function () {
          state.token = null;
          showLogin();
        });
    } else {
      showLogin();
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
  } else {
    start();
  }
})();
