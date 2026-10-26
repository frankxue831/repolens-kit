import { Disposable, Config } from './types';
import { BaseService } from './base-service';

export class UserService extends BaseService implements Disposable {
    private config: Config;

    constructor(config: Config) {
        super();
        this.config = config;
    }

    getUser(id: string): string {
        return `user-${id}`;
    }

    dispose(): void {
        // cleanup
    }
}

export abstract class AbstractHandler extends BaseService {
    abstract handle(input: string): void;
}
