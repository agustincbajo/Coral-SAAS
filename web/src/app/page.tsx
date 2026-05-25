export default function HomePage() {
  return (
    <main className="flex min-h-screen items-center justify-center p-12">
      <div className="max-w-xl space-y-4 text-center">
        <h1 className="text-4xl font-bold">Coral</h1>
        <p className="text-lg opacity-70">
          AI-readable wiki for your codebase. Scaffold stub — see{" "}
          <code className="rounded bg-gray-100 px-1 py-0.5">docs/SAAS-PLAN.md</code>
          .
        </p>
      </div>
    </main>
  );
}
