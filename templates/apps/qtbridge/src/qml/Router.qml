pragma Singleton

import QtQuick
import QtQuick.Controls

QtObject {
    property StackView stack: null

    property string initialRoute: "home"
    property var routes: ({})

    readonly property var flatRoutes: flatten(routes, "")

    function register(tree) {
        const next = {};
        for (const key in routes)
            next[key] = routes[key];
        for (const key in tree)
            next[key] = tree[key];
        routes = next;
    }

    function flatten(node, prefix) {
        let out = [];
        for (const key in node) {
            const value = node[key];
            const path = prefix ? prefix + "/" + key : key;

            if (typeof value === "string") {
                out.push({
                    pattern: path,
                    file: value
                });
            } else if (value && typeof value === "object") {
                if (value.component)
                    out.push({
                        pattern: path,
                        file: value.component
                    });
                if (value.children)
                    out = out.concat(flatten(value.children, path));
            }
        }
        return out;
    }

    function match(path) {
        const parts = String(path).split("?");
        const rawPath = parts[0];
        const query = parts.length > 1 ? parts.slice(1).join("?") : "";

        const segments = rawPath.split("/").filter(function (s) {
            return s.length > 0;
        });

        let best = null;
        let bestScore = Infinity;

        for (let r = 0; r < flatRoutes.length; r++) {
            const pattern = flatRoutes[r].pattern;
            const patternSegments = pattern.split("/").filter(function (s) {
                return s.length > 0;
            });

            const result = matchSegments(patternSegments, segments);
            if (result && result.dynamicCount < bestScore) {
                best = {
                    file: flatRoutes[r].file,
                    params: result.params,
                    pattern: pattern
                };
                bestScore = result.dynamicCount;
            }
        }

        if (!best)
            return null;

        const queryParams = parseQuery(query);
        for (const key in queryParams) {
            if (best.params[key] === undefined)
                best.params[key] = queryParams[key];
        }

        return best;
    }

    function matchSegments(patternSegments, segments) {
        const params = {};
        let dynamicCount = 0;

        for (let i = 0; i < patternSegments.length; i++) {
            const token = patternSegments[i];

            if (token.indexOf("[...") === 0 && token.charAt(token.length - 1) === "]") {
                const key = token.slice(4, -1);
                params[key] = segments.slice(i).map(decodeURIComponent).join("/");
                dynamicCount += 1;
                return {
                    params: params,
                    dynamicCount: dynamicCount
                };
            }

            if (i >= segments.length)
                return null;

            if (token.charAt(0) === "[" && token.charAt(token.length - 1) === "]") {
                const paramKey = token.slice(1, -1);
                params[paramKey] = decodeURIComponent(segments[i]);
                dynamicCount += 1;
                continue;
            }

            if (token !== segments[i])
                return null;
        }

        if (segments.length !== patternSegments.length)
            return null;

        return {
            params: params,
            dynamicCount: dynamicCount
        };
    }

    function parseQuery(query) {
        const out = {};
        if (!query)
            return out;

        const pairs = query.split("&");
        for (let i = 0; i < pairs.length; i++) {
            if (!pairs[i])
                continue;
            const kv = pairs[i].split("=");
            const key = decodeURIComponent(kv[0]);
            const value = kv.length > 1 ? decodeURIComponent(kv.slice(1).join("=")) : "";
            out[key] = value;
        }
        return out;
    }

    function resolve(path) {
        const matched = match(path);

        if (!matched) {
            console.error("Router: unknown route:", path);
            return null;
        }

        return matched;
    }

    function initialize(stackView, routeTree, initial) {
        stack = stackView;

        if (routeTree)
            register(routeTree);
        if (initial)
            initialRoute = initial;

        const matched = resolve(initialRoute);
        if (matched)
            stack.push(matched.file, matched.params);
    }

    function push(path, properties) {
        const matched = resolve(path);
        if (matched)
            stack.push(matched.file, mergeProps(matched.params, properties));
    }

    function replace(path, properties) {
        const matched = resolve(path);
        if (matched)
            stack.replace(matched.file, mergeProps(matched.params, properties));
    }

    function pop() {
        if (stack && stack.depth > 1)
            stack.pop();
    }

    function home() {
        if (!stack)
            return;

        const matched = resolve(initialRoute);
        if (matched) {
            stack.clear();
            stack.push(matched.file, matched.params);
        }
    }

    function mergeProps(params, properties) {
        const merged = {};
        for (const key in params)
            merged[key] = params[key];
        if (properties) {
            for (const key in properties)
                merged[key] = properties[key];
        }
        return merged;
    }
}
