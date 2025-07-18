addEventListener("fetch", (event) => {
  event.respondWith(handleRequest(event.request));
});

async function handleRequest(request) {
  async function MethodNotAllowed(request) {
    return new Response(`Method ${request.method} not allowed.`, {
      status: 405,
      headers: {
        Allow: "GET",
      },
    });
  }
  // Only GET requests work with this proxy.
  if (request.method !== "GET") return MethodNotAllowed(request);

  const url = new URL(request.url);
  const path = url.pathname;

  let targetUrl;

  // Route based on path
  switch (path) {
    case "/":
      targetUrl = "https://mise.jdx.dev/install.sh";
      break;
    case "/zsh":
      targetUrl = "https://mise.jdx.dev/mise.run/zsh";
      break;
    case "/bash":
      targetUrl = "https://mise.jdx.dev/mise.run/bash";
      break;
    case "/fish":
      targetUrl = "https://mise.jdx.dev/mise.run/fish";
      break;
    default:
      return new Response("Not found", { status: 404 });
  }

  const r = await fetch(targetUrl);
  const response = new Response(r.body, r);
  response.headers.set(
    "cache-control",
    "public, max-age=3600, s-maxage=3600, immutable",
  );
  response.headers.set("content-type", "text/plain");
  return response;
}
