import { Bell, Images, LogOut, Plus, Sparkles, UserRound, Zap, type LucideIcon } from "lucide-react";
import { motion } from "motion/react";
import { ReactNode } from "react";
import { authClient } from "../lib/auth-client";
import { track } from "../lib/analytics";
import type { Account, AppRoute, Clone } from "../types";
import { BottomTabBar } from "./BottomTabBar";

const NAV_ITEMS: { id: AppRoute; Icon: LucideIcon; label: string }[] = [
  { id: "blitz", Icon: Zap, label: "Blitz" },
  { id: "create", Icon: Sparkles, label: "Create" },
  { id: "inbox", Icon: Bell, label: "Inbox" },
  { id: "library", Icon: Images, label: "Library" },
  { id: "me", Icon: UserRound, label: "Me" },
];

const titles: Record<AppRoute, string> = {
  blitz: "Blitz",
  create: "Create",
  inbox: "Inbox",
  library: "Library",
  me: "Me",
  clones: "Clones",
  onboarding: "Mirai"
};

export function MobileShell({
  account,
  clones,
  route,
  selectedCloneId,
  children,
  onRoute,
  onClone,
  onSignedOut
}: {
  account: Account;
  clones: Clone[];
  route: AppRoute;
  selectedCloneId: string;
  children: ReactNode;
  onRoute: (route: AppRoute) => void;
  onClone: (id: string) => void;
  onSignedOut: () => void;
}) {
  const avatarChar = (account.user.name || account.user.email || "M")[0].toUpperCase();

  async function handleSignOut() {
    track("sign_out", { source: "sidebar" });
    await authClient.signOut();
    onSignedOut();
  }

  return (
    <motion.div className="mirai-mobile" initial={{ opacity: 0 }} animate={{ opacity: 1 }}>

      {/* Desktop sidebar — hidden on mobile via CSS */}
      <aside className="app-sidebar">
        <div className="app-sidebar__top">
          <a className="sidebar-brand" href="/">
            <span className="sidebar-brand-mark"><Sparkles size={16} /></span>
            <span>Mirai</span>
          </a>
        </div>

        {route !== "onboarding" && (
          <div className="sidebar-clones">
            <p className="sidebar-label">Clones</p>
            {clones.length === 0 ? (
              <button className="sidebar-clone-btn" onClick={() => onRoute("onboarding")}>
                Set up clone
              </button>
            ) : (
              clones.map((clone) => (
                <button
                  key={clone.id}
                  className={`sidebar-clone-btn${selectedCloneId === clone.id ? " active" : ""}`}
                  onClick={() => onClone(clone.id)}
                >
                  {clone.name}
                </button>
              ))
            )}
            <button className="sidebar-clone-btn sidebar-clone-add" onClick={() => onRoute("clones")}>
              <Plus size={13} />
              <span>New clone</span>
            </button>
          </div>
        )}

        <nav className="sidebar-nav">
          <p className="sidebar-label">Menu</p>
          {NAV_ITEMS.map(({ id, Icon, label }) => (
            <button
              key={id}
              className={`sidebar-nav-item${route === id ? " active" : ""}`}
              onClick={() => onRoute(id)}
              aria-current={route === id ? "page" : undefined}
            >
              <Icon size={18} />
              <span>{label}</span>
            </button>
          ))}
        </nav>

        <div className="sidebar-footer">
          <button className="sidebar-user" onClick={() => onRoute("me")}>
            <span className="sidebar-avatar">{avatarChar}</span>
            <div className="sidebar-user__info">
              <span className="sidebar-user__name">{account.user.name || "Creator"}</span>
              <span className="sidebar-user__email">{account.user.email}</span>
            </div>
          </button>
          <button className="sidebar-signout" title="Sign out" onClick={handleSignOut}>
            <LogOut size={16} />
          </button>
        </div>
      </aside>

      {/* Mobile top bar — hidden on desktop via CSS */}
      <header className="mobile-topbar">
        <button className="app-brand-button" title="Account" onClick={() => onRoute("me")}>
          <span className="app-brand-mark"><Sparkles size={16} /></span>
          <span>Mirai</span>
        </button>
        <div>
          <p>{account.user.email || account.user.name || "Creator"}</p>
          <h1>{titles[route]}</h1>
        </div>
        <button className="icon-button" title="New clone" onClick={() => onRoute("clones")}>
          <Plus size={20} />
        </button>
      </header>

      {/* Mobile clone strip — hidden on desktop via CSS */}
      {route !== "onboarding" && (
        <section className="clone-strip" aria-label="Clone selection">
          {clones.length === 0 ? (
            <button className="clone-pill active" onClick={() => onRoute("onboarding")}>Set up clone</button>
          ) : (
            clones.map((clone) => (
              <button
                className={selectedCloneId === clone.id ? "clone-pill active" : "clone-pill"}
                key={clone.id}
                onClick={() => onClone(clone.id)}
              >
                {clone.name}
              </button>
            ))
          )}
        </section>
      )}

      <section className="mobile-content">
        {/* Desktop-only page header — hidden on mobile via CSS */}
        <header className="desktop-page-header">
          <h1>{titles[route]}</h1>
        </header>
        {children}
      </section>

      {/* Mobile floating sign out — hidden on desktop via CSS */}
      {route !== "me" && (
        <button
          className="floating-signout"
          title="Sign out"
          onClick={async () => {
            track("sign_out", { source: "floating" });
            await authClient.signOut();
            onSignedOut();
          }}
        >
          <LogOut size={18} />
        </button>
      )}

      <BottomTabBar route={route} onRoute={onRoute} />
    </motion.div>
  );
}
