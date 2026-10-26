import { UserService } from './service';
import { Config, Status } from './types';

export const createApp = (config: Config): UserService => {
    const service = new UserService(config);
    return service;
};

export function getStatus(): Status {
    return Status.Active;
}
