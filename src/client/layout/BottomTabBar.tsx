import { Bell, Images, Sparkles, UserRound, Zap } from "lucide-react";
import { motion } from "motion/react";
import type { AppRoute } from "../types";

const tabs = [
  ["blitz", Zap, "Blitz"],
  ["create", Sparkles, "Create"],
  ["inbox", Bell, "Inbox"],
  ["library", Images, "Library"],
  ["me", UserRound, "Account"]
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
        <motion.button
          aria-current={route === id ? "page" : undefined}
          className={route === id ? "active" : ""}
          key={id}
          onClick={() => onRoute(id)}
          title={label}
          whileTap={{ scale: 0.94 }}
        >
          <Icon size={20} />
          <span>{label}</span>
        </motion.button>
      ))}
    </nav>
  );
}
