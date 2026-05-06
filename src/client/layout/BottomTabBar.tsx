import { Bell, Images, Sparkles, UserRound, Zap } from "lucide-react";
import type { AppRoute } from "../types";

const tabs = [
  ["blitz", Zap, "Blitz"],
  ["create", Sparkles, "Create"],
  ["inbox", Bell, "Inbox"],
  ["library", Images, "Library"],
  ["me", UserRound, "Me"]
] as const;

export function BottomTabBar({
  route,
  onRoute
}: {
  route: AppRoute;
  onRoute: (route: AppRoute) => void;
}) {
  return (
    <nav className="bottom-tabs" aria-label="Primary">
      {tabs.map(([id, Icon, label]) => (
        <button
          aria-current={route === id ? "page" : undefined}
          className={route === id ? "active" : ""}
          key={id}
          onClick={() => onRoute(id)}
          title={label}
        >
          <Icon size={20} />
          <span>{label}</span>
        </button>
      ))}
    </nav>
  );
}
