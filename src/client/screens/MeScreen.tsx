import {
  BadgeCheck,
  BarChart3,
  CalendarClock,
  CreditCard,
  HelpCircle,
  LifeBuoy,
  Loader2,
  LogOut,
  Mail,
  Settings,
  ShieldCheck,
  Sparkles,
  UserRound,
  WandSparkles
} from "lucide-react";
import { type ReactNode, useEffect, useMemo, useState } from "react";
import { authClient } from "../lib/auth-client";
import { track } from "../lib/analytics";
import { api } from "../lib/api";
import type { Account, GenerationUsage } from "../types";

type UsageBucket = {
  status?: string;
  kind?: string;
  count: number | string;
};

type AccountUsage = {
  clones: UsageBucket[];
  generations: UsageBucket[];
  media: UsageBucket[];
  generationUsage?: GenerationUsage;
};

export function MeScreen({ account, onSignedOut }: { account: Account; onSignedOut: () => void }) {
  const [signingOut, setSigningOut] = useState(false);
  const [usage, setUsage] = useState<AccountUsage | null>(null);

  const displayName = account.user.name || account.user.email || "Creator";
  const initial = displayName[0]?.toUpperCase() || "M";
  const billingServer = account.billing.server || "sandbox";
  const recentEvent = account.billing.recentEvents?.[0];
  const stats = useMemo(
    () => [
      { label: "Clones", value: sumBuckets(usage?.clones) },
      { label: "Generations", value: sumBuckets(usage?.generations) },
      { label: "Assets", value: sumBuckets(usage?.media) }
    ],
    [usage]
  );

  useEffect(() => {
    let ignore = false;
    api<AccountUsage>("/api/account/usage")
      .then((next) => {
        if (!ignore) setUsage(next);
      })
      .catch(() => undefined);
    return () => {
      ignore = true;
    };
  }, []);

  async function signOut() {
    setSigningOut(true);
    track("sign_out", { source: "profile" });
    try {
      await authClient.signOut();
      onSignedOut();
    } finally {
      setSigningOut(false);
    }
  }

  const dailyGenerationMeter = dailyGenerationMeterValue(usage?.generationUsage, stats[1].value);

  return (
    <div className="me-screen account-screen">
      <section className="account-hero">
        <div className="account-avatar" aria-hidden="true">
          {initial}
        </div>
        <div className="account-identity">
          <span className="account-kicker">
            <BadgeCheck size={14} />
            Creator account
          </span>
          <h2>{displayName}</h2>
          <p>{account.user.email || "Email not added"}</p>
          <div className="account-badges" aria-label="Account status">
            <span>Free plan</span>
            <span>Polar {billingServer}</span>
          </div>
        </div>
        <div className="account-stat-grid">
          {stats.map((item) => (
            <div className="account-stat" key={item.label}>
              <strong>{item.value}</strong>
              <span>{item.label}</span>
            </div>
          ))}
        </div>
      </section>

      <div className="account-grid">
        <section className="account-card account-plan-card">
          <div className="account-card-heading">
            <span>Plan</span>
            <strong>Mirai Free</strong>
          </div>
          <p>Create trend-led clone content with starter limits. Upgrade when you need more generations, priority rendering, and advanced models.</p>
          <div className="account-plan-status">
            <span>Billing status</span>
            <strong>{account.billing.polarEnabled ? "Active" : "Local"}</strong>
          </div>
          {recentEvent && (
            <div className="account-plan-status">
              <span>Latest event</span>
              <strong>{recentEvent.event_type}</strong>
            </div>
          )}
          <button
            className="account-primary"
            disabled={!account.billing.checkoutEnabled}
            onClick={() => {
              track("checkout_started", { slug: "pro" });
              void authClient.checkout({ slug: "pro" });
            }}
          >
            <CreditCard size={16} />
            Upgrade to Pro
          </button>
          <button
            className="account-secondary"
            disabled={!account.billing.portalEnabled}
            onClick={() => {
              track("portal_opened");
              void authClient.customer.portal();
            }}
          >
            <CreditCard size={16} />
            Billing Portal
          </button>
        </section>

        <section className="account-card">
          <div className="account-card-heading">
            <span>This month</span>
            <strong>Workspace activity</strong>
          </div>
          <AccountMeter label="Clone profiles" value={stats[0].value} max={5} />
          <AccountMeter
            label="Daily generations"
            value={dailyGenerationMeter.value}
            max={dailyGenerationMeter.max}
          />
          <AccountMeter label="Saved assets" value={stats[2].value} max={48} />
        </section>

        <section className="account-card account-actions-card">
          <div className="account-card-heading">
            <span>Quick actions</span>
            <strong>Account settings</strong>
          </div>
          <AccountAction icon={<UserRound size={18} />} title="Creator identity" description="Name, email, and profile details" />
          <AccountAction icon={<ShieldCheck size={18} />} title="Security" description="Password and connected sign-in methods" />
          <AccountAction icon={<Settings size={18} />} title="Preferences" description="Email, app, and generation defaults" />
        </section>

        <section className="account-card account-support-card">
          <div className="account-card-heading">
            <span>Support</span>
            <strong>Need help?</strong>
          </div>
          <AccountAction icon={<HelpCircle size={18} />} title="Help center" description="Guides for Souls, bubbles, and generations" />
          <a className="account-action" href="mailto:support@phantomsys.dev">
            <span><LifeBuoy size={18} /></span>
            <div>
              <strong>Contact support</strong>
              <small>Get help from the Mirai team</small>
            </div>
          </a>
          <div className="account-system-row">
            <span />
            <div>
              <strong>System status</strong>
              <small>All systems operational</small>
            </div>
          </div>
        </section>

        <section className="account-card account-perks-card">
          <div>
            <span className="account-kicker">
              <Sparkles size={14} />
              Creator perks
            </span>
            <h3>Unlock a faster creator workflow with Pro.</h3>
          </div>
          <div className="account-perks">
            <div><WandSparkles size={18} /><span>Priority rendering</span></div>
            <div><BarChart3 size={18} /><span>Higher monthly limits</span></div>
            <div><CalendarClock size={18} /><span>Early feature access</span></div>
            <div><Mail size={18} /><span>Priority support</span></div>
          </div>
        </section>
      </div>
    </div>
  );
}

function AccountMeter({ label, value, max }: { label: string; value: number; max: number }) {
  const progress = Math.max(4, Math.min(100, (value / max) * 100));

  return (
    <div className="account-meter">
      <div>
        <span>{label}</span>
        <strong>{value} / {max}</strong>
      </div>
      <div className="account-meter-track">
        <span style={{ width: `${progress}%` }} />
      </div>
    </div>
  );
}

function AccountAction({
  icon,
  title,
  description
}: {
  icon: ReactNode;
  title: string;
  description: string;
}) {
  return (
    <div className="account-action">
      <span>{icon}</span>
      <div>
        <strong>{title}</strong>
        <small>{description}</small>
      </div>
    </div>
  );
}

function sumBuckets(buckets: UsageBucket[] | undefined) {
  if (!buckets) return 0;
  return buckets.reduce((total, bucket) => total + Number(bucket.count || 0), 0);
}

export function dailyGenerationMeterValue(generationUsage: GenerationUsage | undefined, generationBucketTotal: number) {
  return {
    value: generationUsage?.imagesToday ?? generationBucketTotal,
    max: generationUsage?.dailyLimit ?? 24
  };
}
