import { AlertIcon } from '../components/icons';

/** Logo, name and one line under it: the head of the login and setup screens. */
export function Brand({ sub }: { sub: string }) {
  return (
    <div className="login__brand">
      <img src="/icon.svg" alt="" width={44} height={44} />
      <div>
        <h1 className="login__title">Nimrag Connect</h1>
        <p className="login__sub">{sub}</p>
      </div>
    </div>
  );
}

/** An error under a form, announced to screen readers. */
export function FormError({ children }: { children: string | null }) {
  if (!children) return null;
  return (
    <p className="login__error" role="alert">
      <AlertIcon size={20} />
      <span>{children}</span>
    </p>
  );
}
