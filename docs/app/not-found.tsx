import Link from "next/link";

export default function NotFound() {
  return (
    <main className="mx-auto max-w-3xl px-4 py-32 text-center">
      <h1 className="text-4xl font-semibold tracking-tight">Page not found</h1>
      <p className="mt-4 text-muted-foreground">That page doesn’t exist or has moved.</p>
      <Link href="/" className="mt-8 inline-block font-medium text-brand">
        Back to the docs →
      </Link>
    </main>
  );
}
