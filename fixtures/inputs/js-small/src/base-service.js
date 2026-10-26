class BaseService {
    constructor() {
        this.initialized = false;
    }

    initialize() {
        this.initialized = true;
    }

    isReady() {
        return this.initialized;
    }
}

module.exports = { BaseService };
