export type Env = CloudflareBindings;

export type AuthUser = {
  id: string;
  email?: string;
  name?: string | null;
  image?: string | null;
};

export type AuthSession = {
  id: string;
  userId: string;
  expiresAt?: Date | string;
};

export type AppVariables = {
  user: AuthUser | null;
  session: AuthSession | null;
  auth: ReturnType<typeof import("./auth").createAuth>;
};

export type AppBindings = {
  Bindings: Env;
  Variables: AppVariables;
};
