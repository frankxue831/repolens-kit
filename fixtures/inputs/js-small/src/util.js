const types = require('./types');

function formatName(name) {
    return name.trim().toLowerCase();
}

function validateConfig(config) {
    return config.host && config.port > 0;
}

module.exports = { formatName, validateConfig };
