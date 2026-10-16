// Node.js HTTP benchmark reference server for TruLang comparison
const http = require('http');
const url = require('url');

const server = http.createServer((req, res) => {
    const parsed = url.parse(req.url, true);

    if (parsed.pathname === '/ping') {
        res.writeHead(200, { 'Content-Type': 'text/plain' });
        res.end('pong');
        return;
    }

    const payload = JSON.stringify({
        status: 'success',
        server: 'nodejs-http',
        user: parsed.query.id || null,
        region_a_reset: 'V8-GC'
    });

    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(payload);
});

server.listen(8082);
