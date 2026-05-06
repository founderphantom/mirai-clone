import { CreditCard, Loader2, LogOut, UserRound } from "lucide-react";
import { useState } from "react";
import { authClient } from "../lib/auth-client";
import { track } from "../lib/analytics";
import type { Account } from "../types";

export function MeScreen({ account, onSignedOut }: { account: Account; onSignedOut: () => void }) {
  const [signingOut, setSigningOut] = useState(false);

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

  return (
    <div className="me-screen">
      <section className="profile-card">
        <UserRound size={24} />
        <h2>{account.user.name || account.user.email}</h2>
        <p>Polar {account.billing.server}</p>
        <button className="primary" onClick={signOut} disabled={signingOut}>
          {signingOut ? <Loader2 className="spin" size={16} /> : <LogOut size={16} />}
          Log out
        </button>
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
