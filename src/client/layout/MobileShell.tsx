import { LogOut, Plus, UserRound } from "lucide-react";
import { ReactNode } from "react";
import { authClient } from "../lib/auth-client";
import { track } from "../lib/analytics";
import type { Account, AppRoute, Clone } from "../types";
import { BottomTabBar } from "./BottomTabBar";

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
  return (
    <main className="mirai-mobile">
      <header className="mobile-topbar">
        <button className="icon-button" title="Account" onClick={() => onRoute("me")}>
          <UserRound size={20} />
        </button>
        <div>
          <p>Mirai</p>
          <h1>{titles[route]}</h1>
        </div>
        <button className="icon-button" title="New clone" onClick={() => onRoute("clones")}>
          <Plus size={20} />
        </button>
      </header>

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

      <section className="mobile-content">{children}</section>

      <button
        className="floating-signout"
        title="Sign out"
        onClick={async () => {
          track("sign_out");
          await authClient.signOut();
          onSignedOut();
        }}
      >
        <LogOut size={18} />
      </button>
      <BottomTabBar route={route} onRoute={onRoute} />
    </main>
  );
}
