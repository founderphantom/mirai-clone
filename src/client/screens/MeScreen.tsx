import { CreditCard, UserRound } from "lucide-react";
import { authClient } from "../lib/auth-client";
import { track } from "../lib/analytics";
import type { Account } from "../types";

export function MeScreen({ account }: { account: Account }) {
  return (
    <div className="me-screen">
      <section className="profile-card">
        <UserRound size={24} />
        <h2>{account.user.name || account.user.email}</h2>
        <p>Polar {account.billing.server}</p>
      </section>
      <section className="plan-card">
        <h2>Free</h2>
        <p>10 starter credits</p>
        <button
          className="primary"
          disabled={!account.billing.checkoutEnabled}
          onClick={() => {
            track("checkout_started", { slug: "pro" });
            void authClient.checkout({ slug: "pro" });
          }}
        >
          <CreditCard size={16} />
          Upgrade
        </button>
        <button
          disabled={!account.billing.portalEnabled}
          onClick={() => {
            track("portal_opened");
            void authClient.customer.portal();
          }}
        >
          Portal
        </button>
      </section>
    </div>
  );
}
